#[cfg(windows)]
pub mod d3d;
#[cfg(windows)]
pub mod encoder;
#[cfg(windows)]
pub mod mf;
#[cfg(windows)]
pub mod pipeline;
#[cfg(windows)]
pub mod preprocessor;

#[cfg(windows)]
use core_types::{EncodeJobQueue, EncodeResult, VideoCodec, VideoEncoderFactory};
#[cfg(windows)]
use std::sync::Arc;
#[cfg(windows)]
use tokio::sync::mpsc as tokio_mpsc;
#[cfg(windows)]
use tracing::{info, warn};

#[cfg(windows)]
use self::mf::check_mf_available;

/// Media Foundation H.264 エンコーダーファクトリ
/// 利用可能でない場合はOpenH264にフォールバック
#[cfg(windows)]
pub struct MediaFoundationH264EncoderFactory {
    use_mf: bool,
}

#[cfg(windows)]
impl MediaFoundationH264EncoderFactory {
    pub fn new() -> Self {
        // Media Foundationが利用可能かチェック
        let use_mf = check_mf_available();
        if use_mf {
            info!("Media Foundation H.264 encoder is available, using MF encoder");
        } else {
            warn!("Media Foundation H.264 encoder is not available, will fallback to OpenH264");
        }
        Self { use_mf }
    }

    pub fn use_media_foundation(&self) -> bool {
        self.use_mf
    }
}

#[cfg(windows)]
impl VideoEncoderFactory for MediaFoundationH264EncoderFactory {
    fn setup(
        &self,
    ) -> (
        Arc<EncodeJobQueue>,
        tokio_mpsc::UnboundedReceiver<EncodeResult>,
    ) {
        if self.use_mf {
            pipeline::start_mf_encode_workers()
        } else {
            // OpenH264にフォールバック
            crate::h264::openh264::start_encode_workers()
        }
    }

    fn codec(&self) -> VideoCodec {
        VideoCodec::H264
    }
}

#[cfg(test)]
#[path = "../mmf_test.rs"]
mod mmf_test;
