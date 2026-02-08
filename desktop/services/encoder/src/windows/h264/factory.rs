use core_types::{EncodeJobSlot, EncodeResult, VideoCodec, VideoEncoderFactory};
use std::sync::Arc;
use tokio::sync::mpsc as tokio_mpsc;
use tracing::info;

use super::pipeline;

/// Media Foundation H.264 エンコーダーファクトリ
pub struct MediaFoundationH264EncoderFactory;

impl MediaFoundationH264EncoderFactory {
    pub fn new() -> Self {
        Self
    }
}

impl VideoEncoderFactory for MediaFoundationH264EncoderFactory {
    fn setup(
        &self,
    ) -> (
        Arc<EncodeJobSlot>,
        tokio_mpsc::UnboundedReceiver<EncodeResult>,
    ) {
        info!("Starting Media Foundation H.264 encoder workers");
        pipeline::start_mf_encode_workers()
    }

    fn codec(&self) -> VideoCodec {
        VideoCodec::H264
    }
}
