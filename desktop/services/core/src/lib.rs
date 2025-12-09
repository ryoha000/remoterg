use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedReceiver;

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
    pub rgb_dur: Duration,
    pub encode_dur: Duration,
    pub pack_dur: Duration,
    pub total_dur: Duration,
    pub sample_size: usize,
}

/// エンコーダーファクトリ（複数ワーカーを生成）
pub trait VideoEncoderFactory: Send + Sync {
    fn start_workers(
        &self,
        worker_count: usize,
        init_width: u32,
        init_height: u32,
    ) -> (
        Vec<std::sync::mpsc::Sender<EncodeJob>>,
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
