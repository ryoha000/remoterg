use anyhow::{Context, Result};
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

impl CodecType {
    /// エンコーダーを作成し、初期設定を実行
    pub fn create_encoder(
        self,
        d3d_resources: crate::windows::utils::d3d::D3D11Resources,
        width: u32,
        height: u32,
    ) -> Result<Box<dyn HardwareEncoder>> {
        use windows::Win32::Media::MediaFoundation::{MFVideoFormat_AV1, MFVideoFormat_H264};

        // エンコーダーを作成
        let encoder: Box<dyn HardwareEncoder> = match self {
            CodecType::H264 => {
                let enc = crate::windows::h264::encoder::H264Encoder::create(d3d_resources)?;
                Box::new(enc)
            }
            CodecType::AV1 => {
                let enc = crate::windows::av1::encoder::AV1Encoder::create(d3d_resources)?;
                Box::new(enc)
            }
        };

        // 共通設定: 低遅延属性
        crate::windows::utils::media_type::setup_low_latency_attributes(encoder.transform())?;

        // 共通設定: メディアタイプ
        let video_format = match self {
            CodecType::H264 => &MFVideoFormat_H264,
            CodecType::AV1 => &MFVideoFormat_AV1,
        };
        crate::windows::utils::media_type::setup_media_types(
            encoder.transform(),
            width,
            height,
            video_format,
        )?;

        Ok(encoder)
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

    /// Start streaming (default implementation provided)
    fn start_streaming(&self) -> Result<()> {
        use windows::Win32::Media::MediaFoundation::{
            MFT_MESSAGE_COMMAND_FLUSH, MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,
            MFT_MESSAGE_NOTIFY_START_OF_STREAM,
        };

        unsafe {
            self.transform()
                .ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0)
                .context("Failed to flush encoder")?;

            self.transform()
                .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
                .context("Failed to notify begin streaming")?;

            self.transform()
                .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
                .context("Failed to notify start of stream")?;

            Ok(())
        }
    }

    /// Retrieve encoded data from output sample
    /// Returns EncodedFrame if successful
    /// This method is responsible for any codec-specific post-processing (e.g. Annex-B conversion)
    fn process_output(
        &mut self,
        sample: &windows::Win32::Media::MediaFoundation::IMFSample,
    ) -> Result<EncodedFrame>;
}
