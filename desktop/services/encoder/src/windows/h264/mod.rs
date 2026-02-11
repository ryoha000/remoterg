pub mod encoder;
pub mod media_type;
pub mod nal;

use core_types::{EncodeJobSlot, EncodeResult, VideoCodec, VideoEncoderFactory};
use std::sync::Arc;
use tokio::sync::mpsc as tokio_mpsc;

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
        crate::windows::pipeline::start_mf_encode_workers(crate::windows::codec::CodecType::H264)
    }

    fn codec(&self) -> VideoCodec {
        VideoCodec::H264
    }
}
