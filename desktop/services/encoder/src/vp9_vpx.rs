use tokio::sync::mpsc as tokio_mpsc;
use vpx_rs::enc::CodecId;

use super::{EncodeJob, EncodeResult, VideoCodec, VideoEncoderFactory};
use crate::vpx_common::start_vpx_encode_workers;

/// VP9 ファクトリ（libvpxベース）
pub struct Vp9EncoderFactory;

impl Vp9EncoderFactory {
    pub fn new() -> Self {
        Self
    }
}

impl VideoEncoderFactory for Vp9EncoderFactory {
    fn start_workers(
        &self,
    ) -> (
        Vec<std::sync::mpsc::Sender<EncodeJob>>,
        tokio_mpsc::UnboundedReceiver<EncodeResult>,
    ) {
        start_vpx_encode_workers(CodecId::VP9, "vp9")
    }

    fn codec(&self) -> VideoCodec {
        VideoCodec::Vp9
    }
}
