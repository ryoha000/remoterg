use anyhow::{Context, Result};
use tracing::warn;
use windows::core::Interface;
use windows::Win32::Media::MediaFoundation::{
    CODECAPI_AVEncVideoForceKeyFrame, ICodecAPI, IMFMediaEventGenerator, IMFTransform,
    MFSampleExtension_CleanPoint, MFT_MESSAGE_COMMAND_FLUSH,
    MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_START_OF_STREAM,
};

use crate::windows::codec::{EncodedFrame, HardwareEncoder};

use crate::windows::utils::d3d::D3D11Resources;

/// 非同期ハードウェア AV1 エンコーダー
pub struct AV1Encoder {
    transform: IMFTransform,
    event_generator: IMFMediaEventGenerator,
    #[allow(dead_code)]
    d3d_resources: D3D11Resources,
    width: u32,
    height: u32,
}

impl AV1Encoder {
    /// AV1 エンコーダーを作成
    pub fn create(d3d_resources: D3D11Resources, width: u32, height: u32) -> Result<Self> {
        let transform = find_async_av1_encoder().context("Failed to find async AV1 encoder MFT")?;

        // D3D マネージャーを設定
        d3d_resources.setup_mft(&transform)?;

        // IMFMediaEventGeneratorを取得
        let event_generator: IMFMediaEventGenerator = transform
            .cast()
            .context("Failed to get IMFMediaEventGenerator from transform")?;

        let encoder = Self {
            transform,
            event_generator,
            d3d_resources,
            width,
            height,
        };

        Ok(encoder)
    }
}


/// 非同期ハードウェア AV1 エンコーダー MFT を検索
pub(crate) fn find_async_av1_encoder() -> Result<IMFTransform> {
    unsafe {
        crate::windows::utils::encoder_finder::find_async_video_encoder(
            core_types::VideoCodec::AV1,
        )
    }
}

unsafe impl Send for AV1Encoder {}

impl HardwareEncoder for AV1Encoder {
    fn transform(&self) -> &IMFTransform {
        &self.transform
    }

    fn event_generator(&self) -> &IMFMediaEventGenerator {
        &self.event_generator
    }

    fn start_streaming(&self) -> Result<()> {
        unsafe {
            self.transform
                .ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0)
                .context("Failed to flush encoder")?;

            self.transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
                .context("Failed to notify begin streaming")?;

            self.transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
                .context("Failed to notify start of stream")?;

            Ok(())
        }
    }

    fn resize(&mut self, width: u32, height: u32) -> Result<()> {
        if self.width != width || self.height != height {
            self.width = width;
            self.height = height;
        }
        Ok(())
    }

    fn set_force_keyframe(&self, force: bool) -> Result<()> {
        unsafe {
            let codec_api: ICodecAPI = self
                .transform
                .cast()
                .context("Failed to cast transform to ICodecAPI")?;

            codec_api
                .SetValue(&CODECAPI_AVEncVideoForceKeyFrame, &force.into())
                .map_err(|e| {
                    anyhow::anyhow!("Failed to set CODECAPI_AVEncVideoForceKeyFrame: {}", e)
                })?;
            Ok(())
        }
    }

    fn get_codec_config(&self) -> Option<Vec<u8>> {
        None
    }

    fn process_output(
        &mut self,
        sample: &windows::Win32::Media::MediaFoundation::IMFSample,
    ) -> Result<EncodedFrame> {
        unsafe {
            let buffer = sample
                .GetBufferByIndex(0)
                .context("Failed to get output buffer")?;

            let mut data_ptr: *mut u8 = std::ptr::null_mut();
            let mut max_length: u32 = 0;
            buffer
                .Lock(&mut data_ptr, Some(&mut max_length), None)
                .context("Failed to lock output buffer")?;

            let current_length = match buffer.GetCurrentLength() {
                Ok(len) => len,
                Err(e) => {
                    let _ = buffer.Unlock();
                    return Err(anyhow::anyhow!("Failed to get output buffer length: {}", e));
                }
            };

            let mut encoded_data = Vec::with_capacity(current_length as usize);
            if current_length > 0 && !data_ptr.is_null() {
                let slice = std::slice::from_raw_parts(data_ptr, current_length as usize);
                encoded_data.extend_from_slice(slice);
            }

            if let Err(e) = buffer.Unlock() {
                warn!("AV1 encoder: failed to unlock output buffer: {}", e);
            }

            // Check for keyframe
            let is_clean_point = match sample.GetUINT32(&MFSampleExtension_CleanPoint) {
                Ok(1) => true,
                _ => false,
            };

            Ok(EncodedFrame {
                data: encoded_data,
                is_keyframe: is_clean_point,
            })
        }
    }
}
