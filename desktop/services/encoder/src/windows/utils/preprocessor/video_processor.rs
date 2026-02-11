use anyhow::{Context, Result};
use std::mem::ManuallyDrop;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Media::MediaFoundation::{
    IMFDXGIBuffer, IMFTransform, MFCreateDXGISurfaceBuffer, MFCreateMediaType, MFCreateSample,
    MFMediaType_Video, MFVideoFormat_ARGB32, MFVideoFormat_NV12, MFVideoInterlace_Progressive,
    MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_OUTPUT_DATA_BUFFER,
    MF_E_TRANSFORM_NEED_MORE_INPUT, MF_E_TRANSFORM_STREAM_CHANGE,
};

/// Media Foundation Video Processor MFT のラッパー
pub struct VideoProcessor {
    transform: IMFTransform,
}

impl VideoProcessor {
    pub fn new() -> Result<Self> {
        let transform = unsafe {
            crate::windows::utils::mf::find_video_processor()
                .context("Video Processor MFT の検索に失敗しました")?
        };
        Ok(Self { transform })
    }

    pub fn transform(&self) -> &IMFTransform {
        &self.transform
    }

    /// ビデオプロセッサのメディアタイプを設定
    pub fn configure(
        &self,
        src_width: u32,
        src_height: u32,
        dst_width: u32,
        dst_height: u32,
    ) -> Result<()> {
        unsafe {
            // 入力メディアタイプ (BGRA)
            let input_media_type = MFCreateMediaType()
                .context("入力メディアタイプの作成に失敗しました")?;

            input_media_type
                .SetGUID(
                    &windows::Win32::Media::MediaFoundation::MF_MT_MAJOR_TYPE,
                    &MFMediaType_Video,
                )
                .context("入力メジャータイプの設定に失敗しました")?;

            input_media_type
                .SetGUID(
                    &windows::Win32::Media::MediaFoundation::MF_MT_SUBTYPE,
                    &MFVideoFormat_ARGB32,
                )
                .context("入力サブタイプの設定に失敗しました")?;

            let frame_size = ((src_width as u64) << 32) | (src_height as u64);
            input_media_type
                .SetUINT64(
                    &windows::Win32::Media::MediaFoundation::MF_MT_FRAME_SIZE,
                    frame_size,
                )
                .context("入力フレームサイズの設定に失敗しました")?;

            let frame_rate = (60u64 << 32) | 1u64;
            input_media_type
                .SetUINT64(
                    &windows::Win32::Media::MediaFoundation::MF_MT_FRAME_RATE,
                    frame_rate,
                )
                .context("入力フレームレートの設定に失敗しました")?;

            input_media_type
                .SetUINT32(
                    &windows::Win32::Media::MediaFoundation::MF_MT_INTERLACE_MODE,
                    MFVideoInterlace_Progressive.0 as u32,
                )
                .context("入力インターレースモードの設定に失敗しました")?;

            self.transform
                .SetInputType(0, &input_media_type, 0)
                .context("Video Processor 入力タイプの設定に失敗しました")?;

            // 出力メディアタイプ (NV12)
            let output_media_type = MFCreateMediaType()
                .context("出力メディアタイプの作成に失敗しました")?;

            output_media_type
                .SetGUID(
                    &windows::Win32::Media::MediaFoundation::MF_MT_MAJOR_TYPE,
                    &MFMediaType_Video,
                )
                .context("出力メジャータイプの設定に失敗しました")?;

            output_media_type
                .SetGUID(
                    &windows::Win32::Media::MediaFoundation::MF_MT_SUBTYPE,
                    &MFVideoFormat_NV12,
                )
                .context("出力サブタイプの設定に失敗しました")?;

            let output_frame_size = ((dst_width as u64) << 32) | (dst_height as u64);
            output_media_type
                .SetUINT64(
                    &windows::Win32::Media::MediaFoundation::MF_MT_FRAME_SIZE,
                    output_frame_size,
                )
                .context("出力フレームサイズの設定に失敗しました")?;

            output_media_type
                .SetUINT64(
                    &windows::Win32::Media::MediaFoundation::MF_MT_FRAME_RATE,
                    frame_rate,
                )
                .context("出力フレームレートの設定に失敗しました")?;

            output_media_type
                .SetUINT32(
                    &windows::Win32::Media::MediaFoundation::MF_MT_INTERLACE_MODE,
                    MFVideoInterlace_Progressive.0 as u32,
                )
                .context("出力インターレースモードの設定に失敗しました")?;

            self.transform
                .SetOutputType(0, &output_media_type, 0)
                .context("Video Processor 出力タイプの設定に失敗しました")?;

            // ストリーム開始通知（非同期MFTで必要）
            self.transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
                .context("ストリーム開始通知に失敗しました")?;

            self.transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
                .context("ストリーム先頭通知に失敗しました")?;

            Ok(())
        }
    }

    /// 入力テクスチャを処理して出力を生成
    pub fn process(
        &self,
        input_texture: ID3D11Texture2D,
        timestamp: i64,
    ) -> Result<Option<ID3D11Texture2D>> {
        unsafe {
            // DXGI Surface Buffer 作成
            let input_buffer = MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, &input_texture, 0, false)
                .map_err(|e| anyhow::anyhow!("DXGI Surface Buffer の作成に失敗しました: {}", e))?;

            // 入力サンプル作成
            let input_sample = MFCreateSample()
                .context("入力サンプルの作成に失敗しました")?;

            input_sample
                .AddBuffer(&input_buffer)
                .context("サンプルへのバッファ追加に失敗しました")?;

            input_sample
                .SetSampleTime(timestamp)
                .context("サンプル時刻の設定に失敗しました")?;

            // ProcessInput
            self.transform
                .ProcessInput(0, &input_sample, 0)
                .context("Video Processor への入力処理に失敗しました")?;

            // ProcessOutput
            let mut output_texture_result: Option<ID3D11Texture2D> = None;

            loop {
                let mut output_data_buffer = MFT_OUTPUT_DATA_BUFFER {
                    dwStreamID: 0,
                    pSample: ManuallyDrop::new(None),
                    dwStatus: 0,
                    pEvents: ManuallyDrop::new(None),
                };
                let mut status: u32 = 0;

                match self.transform.ProcessOutput(
                    0,
                    std::slice::from_mut(&mut output_data_buffer),
                    &mut status,
                ) {
                    Ok(_) => {
                        if let Some(output_sample) = output_data_buffer.pSample.take() {
                            let output_buffer = output_sample
                                .GetBufferByIndex(0)
                                .context("出力バッファの取得に失敗しました")?;

                            if let Ok(dxgi_buffer) = output_buffer.cast::<IMFDXGIBuffer>() {
                                let mut texture_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
                                if dxgi_buffer
                                    .GetResource(&ID3D11Texture2D::IID, &mut texture_ptr as *mut _)
                                    .is_ok()
                                {
                                    if !texture_ptr.is_null() {
                                        #[allow(unused_unsafe)]
                                        let texture = ID3D11Texture2D::from_raw(texture_ptr as _);
                                        output_texture_result = Some(texture);
                                        continue;
                                    }
                                }
                            }
                        }
                    }
                    Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => {
                        break;
                    }
                    Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                        tracing::warn!("Video Processor: ストリーム変更が検出されました");
                        break;
                    }
                    Err(e) => {
                        return Err(anyhow::anyhow!(
                            "ProcessOutput 失敗: {} (code: {:?}, status: {})",
                            e,
                            e.code(),
                            status
                        ));
                    }
                }
            }

            Ok(output_texture_result)
        }
    }
}
