use anyhow::{Context, Result};
use tracing::{debug, warn};
use windows::core::Interface;
use windows::Win32::Media::MediaFoundation::{
    CODECAPI_AVEncVideoForceKeyFrame, ICodecAPI, IMFMediaEventGenerator, IMFTransform,
    MFSampleExtension_CleanPoint, MFT_MESSAGE_COMMAND_FLUSH, MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,
    MFT_MESSAGE_NOTIFY_START_OF_STREAM,
};

use crate::windows::codec::{EncodedFrame, HardwareEncoder};
use crate::windows::h264::media_type;
use crate::windows::h264::nal;
use crate::windows::utils::d3d::D3D11Resources;

/// 非同期ハードウェア H.264 エンコーダー
pub struct H264Encoder {
    transform: IMFTransform,
    event_generator: IMFMediaEventGenerator,
    #[allow(dead_code)]
    d3d_resources: D3D11Resources,
    width: u32,
    height: u32,
    first_keyframe_sent: bool,
}

impl H264Encoder {
    /// H.264 エンコーダーを作成
    pub fn create(d3d_resources: D3D11Resources, width: u32, height: u32) -> Result<Self> {
        unsafe {
            let transform = crate::windows::utils::encoder_finder::find_async_video_encoder(
                core_types::VideoCodec::H264,
            )
            .context("Failed to find async H.264 encoder MFT")?;

            // D3D マネージャーを設定
            d3d_resources.setup_mft(&transform)?;

            // IMFMediaEventGeneratorを取得（非同期MFTのイベント駆動に必要）
            let event_generator: IMFMediaEventGenerator = transform
                .cast()
                .context("Failed to get IMFMediaEventGenerator from transform")?;

            let encoder = Self {
                transform,
                event_generator,
                d3d_resources,
                width,
                height,
                first_keyframe_sent: false,
            };

            // メディアタイプの設定はpipeline側で行うため、ここでは行わない

            Ok(encoder)
        }
    }

    /// 出力メディアタイプからcodec config (SPS/PPS) を取得（best-effort）
    /// 戻り値: (SPS NAL, PPS NAL) - 取得できない場合はNone
    fn get_codec_config_internal(&self) -> Option<(Vec<u8>, Vec<u8>)> {
        media_type::get_codec_config(&self.transform)
    }
}

// Media FoundationのCOMオブジェクトは一般的にスレッドセーフ（特に非同期MFT）
unsafe impl Send for H264Encoder {}

impl HardwareEncoder for H264Encoder {
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
            // メディアタイプの再設定はpipeline側で行われる
        }
        Ok(())
    }

    fn set_force_keyframe(&self, force: bool) -> Result<()> {
        unsafe {
            let codec_api: ICodecAPI = self
                .transform
                .cast()
                .context("Failed to cast transform to ICodecAPI")?;
            // CODECAPI_AVEncVideoForceKeyFrameを設定（値は1）
            codec_api
                .SetValue(&CODECAPI_AVEncVideoForceKeyFrame, &force.into())
                .map_err(|e| {
                    anyhow::anyhow!("Failed to set CODECAPI_AVEncVideoForceKeyFrame: {}", e)
                })?;
            Ok(())
        }
    }

    fn get_codec_config(&self) -> Option<Vec<u8>> {
        // H.264の場合、SPS/PPSをAnnex-B形式で返すこともできるが、
        // 外部に返す必要性が低いため、ここではNoneを返しておくか、
        // もしくは実装するか。
        // 現在の要件ではprocess_output内で処理が完結するため、Noneで良い。
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

            let mut encoded_data = Vec::new();
            if current_length > 0 && !data_ptr.is_null() {
                let slice = std::slice::from_raw_parts(data_ptr, current_length as usize);
                encoded_data.extend_from_slice(slice);
            }

            if let Err(e) = buffer.Unlock() {
                warn!("MF encoder: failed to unlock output buffer: {}", e);
            }

            // Annex-B形式に変換（フォーマット自動判定）
            let (mut sample_data, has_sps_pps_in_data) = nal::annexb_from_mf_data(&encoded_data);

            // キーフレーム判定（MFSampleExtension_CleanPoint + SPS/PPS検出）
            let is_clean_point = match sample.GetUINT32(&MFSampleExtension_CleanPoint) {
                Ok(1) => true,
                Ok(0) => false,
                _ => false, // エラーまたは未設定の場合はfalse
            };
            // SPS/PPSが含まれている場合もキーフレームとして扱う（ブラウザがデコード開始できるように）
            let mut is_keyframe = is_clean_point || has_sps_pps_in_data;

            // in-bandにSPS/PPSが無く、codec configから取得したSPS/PPSがある場合、最初のキーフレームに注入
            if !has_sps_pps_in_data && is_keyframe && !self.first_keyframe_sent {
                // ここでSPS/PPSを取得
                if let Some((ref sps, ref pps)) = self.get_codec_config_internal() {
                    debug!(
                        "MF encoder: injecting SPS/PPS from codec config (SPS: {} bytes, PPS: {} bytes)",
                        sps.len(),
                        pps.len()
                    );
                    const START_CODE: &[u8] = &[0x00, 0x00, 0x00, 0x01];
                    let mut injected_data = Vec::with_capacity(
                        START_CODE.len()
                            + sps.len()
                            + START_CODE.len()
                            + pps.len()
                            + sample_data.len(),
                    );
                    injected_data.extend_from_slice(START_CODE);
                    injected_data.extend_from_slice(sps.as_slice());
                    injected_data.extend_from_slice(START_CODE);
                    injected_data.extend_from_slice(pps.as_slice());
                    injected_data.extend_from_slice(&sample_data);
                    sample_data = injected_data;
                    is_keyframe = true; // 注入後は確実にキーフレーム
                    self.first_keyframe_sent = true;
                }
            } else if has_sps_pps_in_data {
                debug!("MF encoder: detected SPS/PPS in encoded data, marking as keyframe");
                self.first_keyframe_sent = true;
            }

            Ok(EncodedFrame {
                data: sample_data,
                is_keyframe,
            })
        }
    }
}
