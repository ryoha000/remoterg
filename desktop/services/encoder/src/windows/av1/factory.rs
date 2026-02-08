use core_types::{EncodeJobSlot, EncodeResult, VideoCodec, VideoEncoderFactory};
use std::sync::Arc;
use tokio::sync::mpsc as tokio_mpsc;
use tracing::info;

use super::pipeline;

/// Media Foundation AV1 エンコーダーファクトリ
pub struct MediaFoundationAV1EncoderFactory;

impl MediaFoundationAV1EncoderFactory {
    pub fn new() -> Self {
        Self
    }
}

impl VideoEncoderFactory for MediaFoundationAV1EncoderFactory {
    fn setup(
        &self,
    ) -> (
        Arc<EncodeJobSlot>,
        tokio_mpsc::UnboundedReceiver<EncodeResult>,
    ) {
        info!("Starting Media Foundation AV1 encoder workers");
        pipeline::start_mf_encode_workers()
    }

    fn codec(&self) -> VideoCodec {
        VideoCodec::AV1
    }
}
