use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;
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
    pub timestamp: u64,
}

/// ビデオコーデックの種類
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VideoCodec {
    H264,
    Vp8,
    Vp9,
}

impl std::str::FromStr for VideoCodec {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "h264" | "h.264" => Ok(VideoCodec::H264),
            "vp8" => Ok(VideoCodec::Vp8),
            "vp9" => Ok(VideoCodec::Vp9),
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
    pub duration: Duration,
    pub enqueue_at: Instant,
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

/// エンコーダーファクトリ
pub trait VideoEncoderFactory: Send + Sync {
    fn setup(
        &self,
    ) -> (
        std::sync::mpsc::Sender<EncodeJob>,
        UnboundedReceiver<EncodeResult>,
    );

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
