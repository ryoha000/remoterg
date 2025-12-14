use anyhow::{Context, Result};
use std::mem::ManuallyDrop;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Texture2D, D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_SAMPLE_DESC};
use windows::Win32::Media::MediaFoundation::{
    IMFDXGIBuffer, IMFTransform, MFCreateDXGISurfaceBuffer, MFCreateMediaType, MFCreateSample,
    MFMediaType_Video, MFVideoFormat_ARGB32, MFVideoFormat_NV12, MFVideoInterlace_Progressive,
    MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_OUTPUT_DATA_BUFFER,
    MF_E_TRANSFORM_NEED_MORE_INPUT, MF_E_TRANSFORM_STREAM_CHANGE,
};

use crate::h264::mmf::d3d::D3D11Resources;

/// Video Processor MFT による前処理（BGRA → NV12 + リサイズ）
pub struct VideoProcessorPreprocessor {
    transform: IMFTransform,
    d3d_resources: D3D11Resources,
    width: u32,
    height: u32,
    input_texture: Option<ID3D11Texture2D>,
    output_texture: Option<ID3D11Texture2D>,
}

impl VideoProcessorPreprocessor {
    /// Video Processor MFT を作成
    pub fn create(d3d_resources: D3D11Resources, width: u32, height: u32) -> Result<Self> {
        unsafe {
            let transform = crate::h264::mmf::mf::find_video_processor()
                .context("Failed to find Video Processor MFT")?;

            // D3D マネージャーを設定
            d3d_resources.setup_mft(&transform)?;

            let mut preprocessor = Self {
                transform,
                d3d_resources,
                width,
                height,
                input_texture: None,
                output_texture: None,
            };

            // メディアタイプを設定
            preprocessor
                .setup_media_types(width, height)
                .context("Failed to setup media types")?;

            Ok(preprocessor)
        }
    }

    /// メディアタイプを設定
    fn setup_media_types(&mut self, width: u32, height: u32) -> Result<()> {
        unsafe {
            // 入力メディアタイプ（BGRA）
            let input_media_type = MFCreateMediaType()
                .ok()
                .context("Failed to create input media type")?;

            input_media_type
                .SetGUID(
                    &windows::Win32::Media::MediaFoundation::MF_MT_MAJOR_TYPE,
                    &MFMediaType_Video,
                )
                .ok()
                .context("Failed to set input major type")?;

            input_media_type
                .SetGUID(
                    &windows::Win32::Media::MediaFoundation::MF_MT_SUBTYPE,
                    &MFVideoFormat_ARGB32,
                )
                .ok()
                .context("Failed to set input subtype")?;

            let frame_size = ((width as u64) << 32) | (height as u64);
            input_media_type
                .SetUINT64(
                    &windows::Win32::Media::MediaFoundation::MF_MT_FRAME_SIZE,
                    frame_size,
                )
                .ok()
                .context("Failed to set input frame size")?;

            let frame_rate = (60u64 << 32) | 1u64;
            input_media_type
                .SetUINT64(
                    &windows::Win32::Media::MediaFoundation::MF_MT_FRAME_RATE,
                    frame_rate,
                )
                .ok()
                .context("Failed to set input frame rate")?;

            input_media_type
                .SetUINT32(
                    &windows::Win32::Media::MediaFoundation::MF_MT_INTERLACE_MODE,
                    MFVideoInterlace_Progressive.0 as u32,
                )
                .ok()
                .context("Failed to set input interlace mode")?;

            self.transform
                .SetInputType(0, &input_media_type, 0)
                .ok()
                .context("Failed to set Video Processor input type")?;

            // 出力メディアタイプ（NV12）
            let output_media_type = MFCreateMediaType()
                .ok()
                .context("Failed to create output media type")?;

            output_media_type
                .SetGUID(
                    &windows::Win32::Media::MediaFoundation::MF_MT_MAJOR_TYPE,
                    &MFMediaType_Video,
                )
                .ok()
                .context("Failed to set output major type")?;

            output_media_type
                .SetGUID(
                    &windows::Win32::Media::MediaFoundation::MF_MT_SUBTYPE,
                    &MFVideoFormat_NV12,
                )
                .ok()
                .context("Failed to set output subtype")?;

            output_media_type
                .SetUINT64(
                    &windows::Win32::Media::MediaFoundation::MF_MT_FRAME_SIZE,
                    frame_size,
                )
                .ok()
                .context("Failed to set output frame size")?;

            output_media_type
                .SetUINT64(
                    &windows::Win32::Media::MediaFoundation::MF_MT_FRAME_RATE,
                    frame_rate,
                )
                .ok()
                .context("Failed to set output frame rate")?;

            output_media_type
                .SetUINT32(
                    &windows::Win32::Media::MediaFoundation::MF_MT_INTERLACE_MODE,
                    MFVideoInterlace_Progressive.0 as u32,
                )
                .ok()
                .context("Failed to set output interlace mode")?;

            self.transform
                .SetOutputType(0, &output_media_type, 0)
                .ok()
                .context("Failed to set Video Processor output type")?;

            // ストリーム開始を通知（非同期MFTでは BEGIN_STREAMING を先に送る必要がある）
            self.transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
                .ok()
                .context("Failed to notify begin streaming")?;

            self.transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
                .ok()
                .context("Failed to notify start of stream")?;

            Ok(())
        }
    }

    /// 解像度が変更された場合に再設定
    pub fn resize(&mut self, width: u32, height: u32) -> Result<()> {
        if self.width != width || self.height != height {
            self.width = width;
            self.height = height;
            self.input_texture = None;
            self.output_texture = None;
            self.setup_media_types(width, height)
                .context("Failed to resize Video Processor")?;
        }
        Ok(())
    }

    /// BGRA データを D3D11 テクスチャにアップロード
    fn upload_bgra_to_texture(
        &mut self,
        bgra_data: &[u8],
        width: u32,
        height: u32,
    ) -> Result<ID3D11Texture2D> {
        unsafe {
            // テクスチャが存在しないか、サイズが異なる場合は再作成
            let needs_recreate = self.input_texture.is_none() || {
                let mut desc = D3D11_TEXTURE2D_DESC::default();
                self.input_texture.as_ref().unwrap().GetDesc(&mut desc);
                desc.Width != width || desc.Height != height
            };

            if needs_recreate {
                let desc = D3D11_TEXTURE2D_DESC {
                    Width: width,
                    Height: height,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_RENDER_TARGET.0) as u32,
                    CPUAccessFlags: 0,
                    MiscFlags: 0,
                };

                let mut texture: Option<ID3D11Texture2D> = None;
                self.d3d_resources
                    .device
                    .CreateTexture2D(&desc, None, Some(&mut texture))
                    .ok()
                    .context("Failed to create input texture")?;

                self.input_texture = texture;
            }

            let texture = self.input_texture.as_ref().unwrap();

            // CPU から GPU へデータをアップロード
            // 注意: 実際の実装では Map/Unmap または UpdateSubresource を使用
            // ここでは簡略化のため、UpdateSubresource を使用
            let row_pitch = width * 4; // BGRA = 4 bytes per pixel
            let depth_pitch = row_pitch * height;

            self.d3d_resources.context.UpdateSubresource(
                texture,
                0,
                None,
                bgra_data.as_ptr() as _,
                row_pitch as u32,
                depth_pitch as u32,
            );

            Ok(texture.clone())
        }
    }

    /// NV12 出力テクスチャを作成
    fn create_output_texture(&mut self, width: u32, height: u32) -> Result<ID3D11Texture2D> {
        unsafe {
            let needs_recreate = self.output_texture.is_none() || {
                let mut desc = D3D11_TEXTURE2D_DESC::default();
                self.output_texture.as_ref().unwrap().GetDesc(&mut desc);
                desc.Width != width || desc.Height != height
            };

            if needs_recreate {
                let desc = D3D11_TEXTURE2D_DESC {
                    Width: width,
                    Height: height,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: DXGI_FORMAT_NV12,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
                    CPUAccessFlags: 0,
                    MiscFlags: 0,
                };

                let mut texture: Option<ID3D11Texture2D> = None;
                self.d3d_resources
                    .device
                    .CreateTexture2D(&desc, None, Some(&mut texture))
                    .ok()
                    .context("Failed to create output texture")?;

                self.output_texture = texture;
            }

            Ok(self.output_texture.as_ref().unwrap().clone())
        }
    }

    /// BGRA データを処理して NV12 テクスチャを生成
    pub fn process(
        &mut self,
        bgra_data: &[u8],
        width: u32,
        height: u32,
        timestamp: i64,
    ) -> Result<ID3D11Texture2D> {
        unsafe {
            // 解像度が変更された場合は再設定
            self.resize(width, height)?;

            // BGRA を D3D11 テクスチャにアップロード
            let input_texture = self.upload_bgra_to_texture(bgra_data, width, height)?;

            // NV12 出力テクスチャを作成
            let output_texture = self.create_output_texture(width, height)?;

            // DXGI サーフェスバッファを作成
            // MFCreateDXGISurfaceBufferの最初のパラメータはID3D11Texture2DインターフェースのIIDを指定する必要がある
            let input_buffer =
                MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, &input_texture, 0, false)
                    .map_err(|e| {
                        anyhow::anyhow!(
                    "Failed to create DXGI surface buffer (format=ARGB32, width={}, height={}): {}",
                    width,
                    height,
                    e
                )
                    })?;

            // 入力サンプルを作成
            let input_sample = MFCreateSample()
                .ok()
                .context("Failed to create input sample")?;

            input_sample
                .AddBuffer(&input_buffer)
                .ok()
                .context("Failed to add buffer to sample")?;

            input_sample
                .SetSampleTime(timestamp)
                .ok()
                .context("Failed to set sample time")?;

            // ProcessInput
            self.transform
                .ProcessInput(0, &input_sample, 0)
                .ok()
                .context("Failed to process input in Video Processor")?;

            // ProcessOutput で NV12 テクスチャを取得
            // 非同期MFTの場合、ProcessOutputをループで呼び出して
            // MF_E_TRANSFORM_NEED_MORE_INPUTが返されるまで繰り返す
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
                            // 出力サンプルからバッファを取得
                            let output_buffer = output_sample
                                .GetBufferByIndex(0)
                                .ok()
                                .context("Failed to get output buffer")?;

                            // IMFDXGIBufferインターフェースを取得してテクスチャを取り出す
                            if let Ok(dxgi_buffer) = output_buffer.cast::<IMFDXGIBuffer>() {
                                let mut texture_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
                                if dxgi_buffer
                                    .GetResource(&ID3D11Texture2D::IID, &mut texture_ptr as *mut _)
                                    .is_ok()
                                {
                                    if !texture_ptr.is_null() {
                                        // from_raw は unsafe だが、null チェック済みなので安全
                                        #[allow(unused_unsafe)]
                                        let texture =
                                            unsafe { ID3D11Texture2D::from_raw(texture_ptr as _) };
                                        // Video Processor MFTが提供したNV12テクスチャを保存
                                        output_texture_result = Some(texture);
                                        // ループを続けて、すべての出力を取得する
                                        continue;
                                    }
                                }
                            }
                        }
                    }
                    Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => {
                        // すべての出力を取得した - 正常終了
                        break;
                    }
                    Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                        // ストリーム変更が発生した場合は警告を出して続行
                        tracing::warn!("Video Processor: stream change detected");
                        break;
                    }
                    Err(e) => {
                        return Err(anyhow::anyhow!(
                            "ProcessOutput failed: {} (code: {:?}, status: {})",
                            e,
                            e.code(),
                            status
                        ));
                    }
                }
            }

            // Video Processor MFTが提供したテクスチャを返すか、フォールバックとして事前に作成したテクスチャを返す
            Ok(output_texture_result.unwrap_or(output_texture))
        }
    }
}
