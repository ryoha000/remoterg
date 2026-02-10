use crate::windows::h264::pipeline::start_mf_encode_workers;
use core_types::{EncodeJobSlot, EncodeResult, VideoEncoderFactory};
use std::sync::Arc;
use tokio::sync::mpsc as tokio_mpsc;

pub struct MediaFoundationH264EncoderFactory;

impl MediaFoundationH264EncoderFactory {
    pub fn new() -> Self {
        Self
    }

    pub fn use_media_foundation(&self) -> bool {
        true
    }
}

impl VideoEncoderFactory for MediaFoundationH264EncoderFactory {
    fn setup(&self) -> (Arc<EncodeJobSlot>, tokio_mpsc::UnboundedReceiver<EncodeResult>) {
        start_mf_encode_workers()
    }

    fn codec(&self) -> core_types::VideoCodec {
        core_types::VideoCodec::H264
    }
}
