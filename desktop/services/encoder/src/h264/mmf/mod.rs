pub mod d3d;
pub mod encoder;
pub mod mf;
pub mod pipeline;
pub mod preprocessor;

use core_types::{EncodeJobSlot, EncodeResult, VideoCodec, VideoEncoderFactory};
use std::sync::Arc;
use tokio::sync::mpsc as tokio_mpsc;
use tracing::{info, warn};

use self::mf::check_mf_available;

/// Media Foundation H.264 エンコーダーファクトリ
/// 利用可能でない場合はOpenH264にフォールバック
pub struct MediaFoundationH264EncoderFactory {
    use_mf: bool,
}

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

impl VideoEncoderFactory for MediaFoundationH264EncoderFactory {
    fn setup(
        &self,
    ) -> (
        Arc<EncodeJobSlot>,
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
mod test;
