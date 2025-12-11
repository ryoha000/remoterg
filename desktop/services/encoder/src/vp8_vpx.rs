use tokio::sync::mpsc as tokio_mpsc;
use vpx_rs::enc::CodecId;

use super::{EncodeJob, EncodeResult, VideoCodec, VideoEncoderFactory};
use crate::vpx_common::start_vpx_encode_workers;

/// VP8 ファクトリ（libvpxベース）
pub struct Vp8EncoderFactory;

impl Vp8EncoderFactory {
    pub fn new() -> Self {
        Self
    }
}

impl VideoEncoderFactory for Vp8EncoderFactory {
    fn start_workers(
        &self,
        worker_count: usize,
    ) -> (
        Vec<std::sync::mpsc::Sender<EncodeJob>>,
        tokio_mpsc::UnboundedReceiver<EncodeResult>,
    ) {
        start_vpx_encode_workers(CodecId::VP8, "vp8", worker_count)
    }

    fn codec(&self) -> VideoCodec {
        VideoCodec::Vp8
    }
}
