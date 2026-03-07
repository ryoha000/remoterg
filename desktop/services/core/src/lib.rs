use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;
use std::sync::OnceLock;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc::{Receiver, Sender, UnboundedReceiver};

/// キャプチャサイズの指定方法
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureSize {
    /// 元画面サイズを使用
    UseSourceSize,
    /// カスタムサイズを指定
    Custom { width: u32, height: u32 },
}

/// Capture の初期設定/変更パラメータ
#[derive(Debug, Clone)]
pub struct CaptureConfig {
    pub size: CaptureSize,
    pub fps: u32,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            size: CaptureSize::UseSourceSize,
            fps: 45,
        }
    }
}

/// Capture サービスへのメッセージ
#[derive(Debug)]
pub enum CaptureMessage {
    Start {
        hwnd: u64,
    },
    Stop,
    UpdateConfig {
        size: CaptureSize,
        fps: u32,
    },
    GetScreenshot {
        tx: tokio::sync::oneshot::Sender<ScreenshotFrame>,
    },
}

/// Capture サービスの実行結果 Future 型
pub type CaptureFuture = Pin<Box<dyn Future<Output = Result<()>> + Send>>;

pub type CaptureFrameSender = Sender<Frame>;
pub type CaptureCommandReceiver = Receiver<CaptureMessage>;

/// レイテンシ計測用の単調時刻基準 (hostd起動時に設定)
static LATENCY_APP_START: OnceLock<Instant> = OnceLock::new();
/// レイテンシ計測用の壁時計基準 (LATENCY_APP_START と同時に記録)
static LATENCY_APP_START_SYSTEM: OnceLock<SystemTime> = OnceLock::new();

/// 単調増加時刻をミリ秒で返す (設定されていない場合は0)
pub fn latency_monotonic_ms() -> f64 {
    LATENCY_APP_START
        .get()
        .map(|s| s.elapsed().as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

/// レイテンシ計測用の基準時刻を設定 (hostd起動時に1回のみ)
pub fn set_latency_app_start(start: Instant) {
    let _ = LATENCY_APP_START.set(start);
    let _ = LATENCY_APP_START_SYSTEM.set(SystemTime::now());
}

/// hostd monotonic ms を SystemTime に変換する
pub fn hostd_mono_ms_to_system_time(mono_ms: f64) -> SystemTime {
    let system_start = LATENCY_APP_START_SYSTEM
        .get()
        .copied()
        .unwrap_or(UNIX_EPOCH);
    let elapsed = Duration::from_secs_f64(mono_ms / 1000.0);
    system_start + elapsed
}

/// NTP エポック (1900-01-01) と Unix エポック (1970-01-01) の差 (秒)
const NTP_EPOCH_OFFSET: u64 = 2_208_988_800;

/// SystemTime を NTP タイムスタンプ (UQ32.32 固定小数点, 64bit) に変換する
pub fn system_time_to_ntp(t: SystemTime) -> u64 {
    let unix_dur = t.duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO);
    let ntp_secs = unix_dur.as_secs() + NTP_EPOCH_OFFSET;
    // 小数部を 32bit 固定小数点に変換 (nanos / 1e9 * 2^32)
    let frac = ((unix_dur.subsec_nanos() as u64) << 32) / 1_000_000_000;
    (ntp_secs << 32) | frac
}

/// キャプチャフレーム (GPU texture handle のみ)
#[derive(Debug, Clone)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub windows_timespan: u64,
    pub id: u64,
    pub texture_handle: Option<u64>,
    /// レイテンシ計測用 (hostd monotonic ms, キャプチャ直後)
    pub t_cap_mono_ms: Option<f64>,
}

/// スクリーンショット用のフレームデータ (CPU buffer を含む)
#[derive(Debug, Clone)]
pub struct ScreenshotFrame {
    pub width: u32,
    pub height: u32,
    pub data: Arc<Vec<u8>>, // RGBA CPU buffer
    pub timestamp: u64,
}

/// ビデオコーデックの種類
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VideoCodec {
    H264,
    AV1,
}

impl std::str::FromStr for VideoCodec {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "h264" | "h.264" => Ok(VideoCodec::H264),
            "av1" => Ok(VideoCodec::AV1),
            other => Err(format!("unsupported codec string: {}", other)),
        }
    }
}

/// エンコード要求
#[derive(Debug)]
pub struct EncodeJob {
    pub width: u32,
    pub height: u32,
    pub timestamp: u64,
    pub enqueue_at: Instant,
    pub request_keyframe: bool,
    pub frame_id: u64,
    pub texture_handle: Option<u64>,
    /// レイテンシ計測用 (hostd monotonic ms)
    pub t_cap_mono_ms: Option<f64>,
    pub t_enc_in_mono_ms: Option<f64>,
}

/// エンコード結果
#[derive(Debug)]
pub struct EncodeResult {
    pub sample_data: Vec<u8>,
    pub is_keyframe: bool,
    pub duration: Duration,
    pub width: u32,
    pub height: u32,
    pub frame_id: u64,
    /// レイテンシ計測用 (hostd monotonic ms)
    pub t_cap_ms: Option<f64>,
    pub t_enc_in_ms: Option<f64>,
    pub t_enc_out_ms: Option<f64>,
}

/// エンコードジョブスロットのシャットダウンエラー
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShutdownError;

impl std::fmt::Display for ShutdownError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "EncodeJobSlot has been shut down")
    }
}

impl std::error::Error for ShutdownError {}

/// エンコードジョブスロット（Dumb Workerパターン用）
/// 最新のフレームのみを保持し、古いフレームは自動的にドロップされる
#[derive(Debug)]
pub struct EncodeJobSlot {
    job: Mutex<Option<EncodeJob>>,
    condvar: Condvar,
    shutdown: Mutex<bool>,
}

impl EncodeJobSlot {
    /// 新しいスロットを作成
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            job: Mutex::new(None),
            condvar: Condvar::new(),
            shutdown: Mutex::new(false),
        })
    }

    /// シャットダウンを通知する
    /// すべての待機中のスレッドを起こし、`take()`が`ShutdownError`を返すようにする
    /// このメソッドは即座に返り、ワーカースレッドの終了を待たない
    pub fn shutdown(&self) {
        let mut shutdown_guard = self.shutdown.lock().unwrap();
        *shutdown_guard = true;
        drop(shutdown_guard);
        self.condvar.notify_all();
    }

    /// 最新のジョブをセット（古いものを置き換え）
    /// 常に成功する（スロットが満杯になることがない）
    pub fn set(&self, job: EncodeJob) {
        let mut guard = self.job.lock().unwrap();
        *guard = Some(job);
        self.condvar.notify_one();
    }

    /// ブロッキングでジョブを取得
    /// ジョブが利用可能になるまで待機する
    /// シャットダウンされた場合は`ShutdownError`を返す
    pub fn take(&self) -> Result<EncodeJob, ShutdownError> {
        let mut guard = self.job.lock().unwrap();
        loop {
            // シャットダウンチェック
            if *self.shutdown.lock().unwrap() {
                return Err(ShutdownError);
            }

            if let Some(job) = guard.take() {
                return Ok(job);
            }

            guard = self.condvar.wait(guard).unwrap();

            // wait()の後にもシャットダウンチェック
            if *self.shutdown.lock().unwrap() {
                return Err(ShutdownError);
            }
        }
    }

    /// ノンブロッキングでジョブを取得
    /// ジョブが利用可能な場合は`Some(EncodeJob)`を返し、そうでない場合は`None`を返す
    /// シャットダウンされた場合は`Some(Err(ShutdownError))`を返す
    pub fn try_take(&self) -> Option<Result<EncodeJob, ShutdownError>> {
        let mut guard = self.job.lock().unwrap();

        // シャットダウンチェック
        if *self.shutdown.lock().unwrap() {
            return Some(Err(ShutdownError));
        }

        guard.take().map(Ok)
    }
}

/// エンコーダーファクトリ
pub trait VideoEncoderFactory: Send + Sync {
    fn setup(&self) -> (Arc<EncodeJobSlot>, UnboundedReceiver<EncodeResult>);

    /// 利用するビデオコーデック
    fn codec(&self) -> VideoCodec;
}

/// WebRTCサービスへのリクエストメッセージ
#[derive(Debug, Clone)]
pub enum WebRtcMessage {
    SetOffer {
        sdp: String,
        codec: Option<VideoCodec>,
    },
    AddIceCandidate {
        candidate: String,
        sdp_mid: Option<String>,
        sdp_mline_index: Option<u16>,
        username_fragment: Option<String>,
    },
    /// ICE Restartをトリガー
    TriggerIceRestart,
    /// ICE RestartのAnswerを受信
    SetAnswerForRestart { sdp: String },
}

/// シグナリングサービスへの応答メッセージ
#[derive(Debug, Clone)]
pub enum SignalingResponse {
    Answer {
        sdp: String,
    },
    Error {
        message: String,
    },
    IceCandidate {
        candidate: String,
        sdp_mid: Option<String>,
        sdp_mline_index: Option<u16>,
        username_fragment: Option<String>,
    },
    IceCandidateComplete,
    /// ICE Restartのための新しいOffer
    OfferForRestart {
        sdp: String,
    },
}

/// DataChannel経由でやり取りするメッセージ
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DataChannelMessage {
    Key {
        key: String,
        down: bool,
    },
    MouseWheel {
        delta: i32,
    },
    ScreenshotRequest {
        include_image: bool,
    },
    Ping {
        timestamp: u64,
    },
    Pong {
        timestamp: u64,
    },
    /// 時刻同期要求 (client -> hostd)
    #[serde(rename = "sync_req")]
    SyncReq {
        seq: u32,
        c1: f64,
    },
    /// 時刻同期応答 (hostd -> client)
    #[serde(rename = "sync_res")]
    SyncRes {
        seq: u32,
        c1: f64,
        s2: f64,
        s3: f64,
    },
    /// フレーム計測サンプル (hostd -> client)
    #[serde(rename = "frame_sample")]
    FrameSample {
        seq: u64,
        frame_id: u64,
        t_cap: f64,
        t_enc_in: f64,
        t_enc_out: f64,
        t_send: f64,
        /// キャプチャ時刻 (Unix ミリ秒)
        capture_unix_ms: i64,
    },
    // Input
    MouseClick {
        x: f64,
        y: f64,
        button: String,
    },
    CursorMove {
        dx: i32,
        dy: i32,
    },
    CursorClick {
        button: String,
    },
    // LLM Analysis
    AnalyzeRequest {
        id: String,
        max_edge: u32,
    },
    // Outgoing messages (Host -> Client)
    #[serde(rename = "SCREENSHOT_METADATA")]
    ScreenshotMetadata {
        payload: ScreenshotMetadataPayload,
    },
    #[serde(rename = "ANALYZE_RESPONSE")]
    AnalyzeResponse {
        id: String,
        text: String,
    },
    #[serde(rename = "ANALYZE_RESPONSE_CHUNK")]
    AnalyzeResponseChunk {
        id: String,
        delta: String,
    },
    #[serde(rename = "ANALYZE_RESPONSE_DONE")]
    AnalyzeResponseDone {
        id: String,
    },
    // LLM Config
    GetLlmConfig,
    UpdateLlmConfig {
        config: LlmConfig,
    },
    LlmConfigResponse {
        config: LlmConfig,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LlmConfig {
    pub port: u16,
    pub model_path: Option<String>,
    pub mmproj_path: Option<String>,
}

#[derive(Debug)]
pub enum TaggerCommand {
    UpdateConfig {
        config: LlmConfig,
    },
    GetConfig {
        reply_tx: tokio::sync::oneshot::Sender<LlmConfig>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenshotMetadataPayload {
    pub id: String,
    pub timestamp: u64,
    pub format: String,
    pub width: u32,
    pub height: u32,
    pub size: u32,
    pub window_title: Option<String>,
    pub process_path: Option<String>,
    pub process_name: Option<String>,
    pub vndb_id: Option<String>,
    pub official_title: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ScreenshotChunk {
    pub id: String,
    pub seq: u32,
    pub total: u32,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub enum OutgoingDataChannelMessage {
    Text(DataChannelMessage),
    Binary(Vec<u8>),
}

#[cfg(test)]
mod tests {
    use super::DataChannelMessage;

    #[test]
    fn sync_res_json_roundtrip_without_unix_anchor_fields() {
        let msg = DataChannelMessage::SyncRes {
            seq: 7,
            c1: 10.0,
            s2: 20.0,
            s3: 21.0,
        };

        let json = serde_json::to_string(&msg).expect("serialize sync_res");
        assert!(json.contains("\"seq\":7"));
        assert!(json.contains("\"c1\":10.0"));
        assert!(json.contains("\"s2\":20.0"));
        assert!(json.contains("\"s3\":21.0"));
        assert!(!json.contains("u2_ms"));
        assert!(!json.contains("u3_ms"));

        let decoded: DataChannelMessage = serde_json::from_str(&json).expect("deserialize sync_res");
        match decoded {
            DataChannelMessage::SyncRes {
                seq,
                c1,
                s2,
                s3,
            } => {
                assert_eq!(seq, 7);
                assert_eq!(c1, 10.0);
                assert_eq!(s2, 20.0);
                assert_eq!(s3, 21.0);
            }
            _ => panic!("unexpected variant"),
        }
    }
}

/// Capture 実装の共通トレイト
pub trait CaptureBackend: Send {
    fn new(frame_tx: CaptureFrameSender, command_rx: CaptureCommandReceiver) -> Self
    where
        Self: Sized;

    fn run(self) -> CaptureFuture;
}

/// 音声フレーム
#[derive(Debug, Clone)]
pub struct AudioFrame {
    pub samples: Vec<f32>, // インターリーブPCM（L,R,L,R,...）
    pub sample_rate: u32,  // 48000
    pub channels: u16,     // 2
    pub timestamp_us: u64, // マイクロ秒タイムスタンプ
}

/// 音声キャプチャサービスへのメッセージ
#[derive(Debug, Clone)]
pub enum AudioCaptureMessage {
    Start { hwnd: u64 },
    Stop,
}

pub type AudioFrameSender = Sender<AudioFrame>;
pub type AudioCaptureCommandReceiver = Receiver<AudioCaptureMessage>;

/// 音声エンコード結果
#[derive(Debug)]
pub struct AudioEncodeResult {
    pub encoded_data: Vec<u8>, // Opusエンコード済みデータ
    pub duration: Duration,    // フレームの長さ（10ms）
    pub is_silent: bool,       // 無音フレームかどうか
}

/// 音声エンコーダーファクトリ
pub trait AudioEncoderFactory: Send + Sync {
    /// エンコード済みデータの受信チャンネルを返す
    /// 音声フレームを送信するチャンネルを返す
    fn setup(&self) -> (Sender<AudioFrame>, UnboundedReceiver<AudioEncodeResult>);
}

/// ビデオストリームサービスへの制御メッセージ
#[derive(Debug, Clone)]
pub enum VideoStreamMessage {
    /// キーフレーム要求 (PLI/FIR RTCP feedback)
    RequestKeyframe,
}
