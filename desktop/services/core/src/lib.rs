use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
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
#[derive(Debug, Clone)]
pub enum CaptureMessage {
    Start { hwnd: u64 },
    Stop,
    UpdateConfig { size: CaptureSize, fps: u32 },
}

/// Capture サービスの実行結果 Future 型
pub type CaptureFuture = Pin<Box<dyn Future<Output = Result<()>> + Send>>;

pub type CaptureFrameSender = Sender<Frame>;
pub type CaptureCommandReceiver = Receiver<CaptureMessage>;

/// キャプチャフレーム
#[derive(Debug, Clone)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
    pub windows_timespan: u64,
}

/// ビデオコーデックの種類
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VideoCodec {
    H264,
}

impl std::str::FromStr for VideoCodec {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "h264" | "h.264" => Ok(VideoCodec::H264),
            other => Err(format!("unsupported codec string: {}", other)),
        }
    }
}

/// エンコード要求
#[derive(Debug)]
pub struct EncodeJob {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub timestamp: u64,
    pub enqueue_at: Instant,
    pub request_keyframe: bool,
}

/// エンコード結果
#[derive(Debug)]
pub struct EncodeResult {
    pub sample_data: Vec<u8>,
    pub is_keyframe: bool,
    pub duration: Duration,
    pub width: u32,
    pub height: u32,
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
    },
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
    },
}

/// DataChannel経由でやり取りするメッセージ
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DataChannelMessage {
    Key { key: String, down: bool },
    MouseWheel { delta: i32 },
    ScreenshotRequest,
}

/// Capture 実装の共通トレイト
pub trait CaptureBackend: Send {
    fn new(frame_tx: CaptureFrameSender, command_rx: CaptureCommandReceiver) -> Self
    where
        Self: Sized;

    fn run(self) -> CaptureFuture;
}
