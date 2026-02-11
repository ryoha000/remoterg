use anyhow::Result;
use core_types::VideoCodec;

use windows::Win32::Media::MediaFoundation::{IMFMediaEventGenerator, IMFTransform};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodecType {
    H264,
    AV1,
}

impl From<CodecType> for VideoCodec {
    fn from(val: CodecType) -> Self {
        match val {
            CodecType::H264 => VideoCodec::H264,
            CodecType::AV1 => VideoCodec::AV1,
        }
    }
}

pub struct EncodedFrame {
    pub data: Vec<u8>,
    pub is_keyframe: bool,
}

/// Hardware Encoder Trait
/// generic interface for Media Foundation hardware encoders (H.264, AV1, etc.)
pub trait HardwareEncoder: Send {
    /// Get the referenced IMFTransform (used for event loop)
    fn transform(&self) -> &IMFTransform;

    /// Get the referenced IMFMediaEventGenerator (used for event loop)
    fn event_generator(&self) -> &IMFMediaEventGenerator;

    /// Start streaming
    fn start_streaming(&self) -> Result<()>;

    /// Resize encoder
    fn resize(&mut self, width: u32, height: u32) -> Result<()>;

    /// Force keyframe for next frame
    fn set_force_keyframe(&self, force: bool) -> Result<()>;

    /// Retrieve encoded data from output sample
    /// Returns EncodedFrame if successful
    /// This method is responsible for any codec-specific post-processing (e.g. Annex-B conversion)
    fn process_output(
        &mut self,
        sample: &windows::Win32::Media::MediaFoundation::IMFSample,
    ) -> Result<EncodedFrame>;

    /// Optional: Get codec specific configuration (SPS/PPS for H.264)
    /// This might be needed for initialization or first keyframe injection
    fn get_codec_config(&self) -> Option<Vec<u8>>;
}
