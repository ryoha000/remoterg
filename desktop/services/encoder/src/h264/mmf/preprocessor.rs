use anyhow::{Context, Result};
use std::mem::ManuallyDrop;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11ComputeShader, ID3D11ShaderResourceView, ID3D11UnorderedAccessView,
    D3D11_BIND_UNORDERED_ACCESS,
};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Texture2D, D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_NV12, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Media::MediaFoundation::{
    IMFDXGIBuffer, IMFTransform, MFCreateDXGISurfaceBuffer, MFCreateMediaType, MFCreateSample,
    MFMediaType_Video, MFVideoFormat_ARGB32, MFVideoFormat_NV12, MFVideoInterlace_Progressive,
    MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_OUTPUT_DATA_BUFFER,
    MF_E_TRANSFORM_NEED_MORE_INPUT, MF_E_TRANSFORM_STREAM_CHANGE,
};

use crate::h264::mmf::d3d::D3D11Resources;

/// Video Processor MFT による前処理（RGBA → BGRA → NV12 + リサイズ）
pub struct VideoProcessorPreprocessor {
    transform: IMFTransform,
    d3d_resources: D3D11Resources,
    width: u32,
    height: u32,
    rgba_texture: Option<ID3D11Texture2D>,
    bgra_texture: Option<ID3D11Texture2D>,
    output_texture: Option<ID3D11Texture2D>,
    rgba_srv: Option<ID3D11ShaderResourceView>,
    bgra_uav: Option<ID3D11UnorderedAccessView>,
    compute_shader: Option<ID3D11ComputeShader>,
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
                rgba_texture: None,
                bgra_texture: None,
                output_texture: None,
                rgba_srv: None,
                bgra_uav: None,
                compute_shader: None,
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
            self.rgba_texture = None;
            self.bgra_texture = None;
            self.output_texture = None;
            self.rgba_srv = None;
            self.bgra_uav = None;
            self.setup_media_types(width, height)
                .context("Failed to resize Video Processor")?;
        }
        Ok(())
    }

    /// Compute Shaderを作成
    fn create_compute_shader(&mut self) -> Result<()> {
        if self.compute_shader.is_some() {
            return Ok(());
        }

        unsafe {
            // HLSL Compute Shaderコード（RGBA→BGRA変換）
            let shader_code = r#"
                Texture2D<float4> rgba_texture : register(t0);
                RWTexture2D<float4> bgra_texture : register(u0);

                [numthreads(8, 8, 1)]
                void CSMain(uint3 id : SV_DispatchThreadID)
                {
                    float4 rgba = rgba_texture[id.xy];
                    // GPU が BGRA テクスチャへの書き込み時に自動変換
                    bgra_texture[id.xy] = rgba;
                }
            "#;

            use windows::core::PCSTR;
            use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
            use windows::Win32::Graphics::Direct3D::Fxc::D3DCOMPILE_OPTIMIZATION_LEVEL3;

            let shader_code_bytes = shader_code.as_bytes();
            let entry_point_bytes = b"CSMain\0";
            let target_bytes = b"cs_5_0\0";
            let entry_point = PCSTR(entry_point_bytes.as_ptr());
            let target = PCSTR(target_bytes.as_ptr());

            let mut compiled_shader: Option<windows::Win32::Graphics::Direct3D::ID3DBlob> = None;
            let mut error_blob: Option<windows::Win32::Graphics::Direct3D::ID3DBlob> = None;

            let result = D3DCompile(
                shader_code_bytes.as_ptr() as _,
                shader_code_bytes.len(),
                None,
                None,
                None,
                entry_point,
                target,
                D3DCOMPILE_OPTIMIZATION_LEVEL3 as u32,
                0,
                &mut compiled_shader,
                Some(&mut error_blob),
            );

            if result.is_err() {
                let error_msg = if let Some(blob) = error_blob.as_ref() {
                    let ptr = blob.GetBufferPointer();
                    let len = blob.GetBufferSize();
                    std::str::from_utf8(std::slice::from_raw_parts(ptr as *const u8, len as usize))
                        .unwrap_or("Unknown error")
                } else {
                    "Unknown error"
                };
                return Err(anyhow::anyhow!(
                    "Failed to compile compute shader: {}",
                    error_msg
                ));
            }

            let compiled_shader = compiled_shader.ok_or_else(|| {
                anyhow::anyhow!("Failed to compile compute shader: compiled_shader is None")
            })?;

            let buffer_ptr = compiled_shader.GetBufferPointer();
            let buffer_size = compiled_shader.GetBufferSize();
            let shader_bytes = std::slice::from_raw_parts(buffer_ptr as *const u8, buffer_size);

            let mut compute_shader: Option<ID3D11ComputeShader> = None;
            self.d3d_resources
                .device
                .CreateComputeShader(shader_bytes, None, Some(&mut compute_shader))
                .ok()
                .context("Failed to create compute shader")?;

            self.compute_shader = compute_shader;
            Ok(())
        }
    }

    /// RGBA データを D3D11 テクスチャにアップロード
    fn upload_rgba_to_texture(
        &mut self,
        rgba_data: &[u8],
        width: u32,
        height: u32,
    ) -> Result<ID3D11Texture2D> {
        unsafe {
            // テクスチャが存在しないか、サイズが異なる場合は再作成
            let needs_recreate = self.rgba_texture.is_none() || {
                let mut desc = D3D11_TEXTURE2D_DESC::default();
                self.rgba_texture.as_ref().unwrap().GetDesc(&mut desc);
                desc.Width != width || desc.Height != height
            };

            if needs_recreate {
                let desc = D3D11_TEXTURE2D_DESC {
                    Width: width,
                    Height: height,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_UNORDERED_ACCESS.0)
                        as u32,
                    CPUAccessFlags: 0,
                    MiscFlags: 0,
                };

                let mut texture: Option<ID3D11Texture2D> = None;
                self.d3d_resources
                    .device
                    .CreateTexture2D(&desc, None, Some(&mut texture))
                    .ok()
                    .context("Failed to create RGBA texture")?;

                self.rgba_texture = texture;
            }

            let texture = self.rgba_texture.as_ref().unwrap();

            // CPU から GPU へデータをアップロード
            let row_pitch = width * 4; // RGBA = 4 bytes per pixel
            let depth_pitch = row_pitch * height;

            self.d3d_resources.context.UpdateSubresource(
                texture,
                0,
                None,
                rgba_data.as_ptr() as _,
                row_pitch as u32,
                depth_pitch as u32,
            );

            Ok(texture.clone())
        }
    }

    /// BGRA テクスチャを作成（GPU側でRGBA→BGRA変換を行う）
    fn create_bgra_texture(&mut self, width: u32, height: u32) -> Result<ID3D11Texture2D> {
        unsafe {
            let needs_recreate = self.bgra_texture.is_none() || {
                let mut desc = D3D11_TEXTURE2D_DESC::default();
                self.bgra_texture.as_ref().unwrap().GetDesc(&mut desc);
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
                    BindFlags: (D3D11_BIND_SHADER_RESOURCE.0
                        | D3D11_BIND_RENDER_TARGET.0
                        | D3D11_BIND_UNORDERED_ACCESS.0) as u32,
                    CPUAccessFlags: 0,
                    MiscFlags: 0,
                };

                let mut texture: Option<ID3D11Texture2D> = None;
                self.d3d_resources
                    .device
                    .CreateTexture2D(&desc, None, Some(&mut texture))
                    .ok()
                    .context("Failed to create BGRA texture")?;

                self.bgra_texture = texture;
            }

            Ok(self.bgra_texture.as_ref().unwrap().clone())
        }
    }

    /// GPU側でRGBA→BGRA変換を行う（Compute Shaderを使用）
    fn convert_rgba_to_bgra(
        &mut self,
        rgba_texture: &ID3D11Texture2D,
        bgra_texture: &ID3D11Texture2D,
        width: u32,
        height: u32,
    ) -> Result<()> {
        unsafe {
            // Compute Shaderを作成（初回のみ）
            self.create_compute_shader()?;

            // RGBAテクスチャのSRVを作成
            if self.rgba_srv.is_none() {
                let mut srv: Option<ID3D11ShaderResourceView> = None;
                self.d3d_resources
                    .device
                    .CreateShaderResourceView(rgba_texture, None, Some(&mut srv))
                    .ok()
                    .context("Failed to create RGBA SRV")?;

                self.rgba_srv = srv;
            }

            // BGRAテクスチャのUAVを作成
            if self.bgra_uav.is_none() {
                let mut uav: Option<ID3D11UnorderedAccessView> = None;
                self.d3d_resources
                    .device
                    .CreateUnorderedAccessView(bgra_texture, None, Some(&mut uav))
                    .ok()
                    .context("Failed to create BGRA UAV")?;

                self.bgra_uav = uav;
            }

            // Compute Shaderを設定
            self.d3d_resources
                .context
                .CSSetShader(self.compute_shader.as_ref(), None);

            // SRVとUAVを設定
            let srv_slice: [Option<ID3D11ShaderResourceView>; 1] = [self.rgba_srv.clone()];
            let uav_slice: [Option<ID3D11UnorderedAccessView>; 1] = [self.bgra_uav.clone()];
            let uav_initial_counts: [u32; 1] = [0];
            self.d3d_resources
                .context
                .CSSetShaderResources(0, Some(&srv_slice));
            self.d3d_resources.context.CSSetUnorderedAccessViews(
                0,
                1,
                Some(uav_slice.as_ptr()),
                Some(uav_initial_counts.as_ptr()),
            );

            // Compute Shaderを実行
            let thread_group_x = (width + 7) / 8;
            let thread_group_y = (height + 7) / 8;
            self.d3d_resources
                .context
                .Dispatch(thread_group_x, thread_group_y, 1);

            // リソースをクリア
            self.d3d_resources.context.CSSetShader(None, None);
            let null_srv_slice: [Option<ID3D11ShaderResourceView>; 1] = [None];
            let null_uav_slice: [Option<ID3D11UnorderedAccessView>; 1] = [None];
            let null_uav_initial_counts: [u32; 1] = [0];
            self.d3d_resources
                .context
                .CSSetShaderResources(0, Some(&null_srv_slice));
            self.d3d_resources.context.CSSetUnorderedAccessViews(
                0,
                1,
                Some(null_uav_slice.as_ptr()),
                Some(null_uav_initial_counts.as_ptr()),
            );

            Ok(())
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

    /// RGBA データを処理して NV12 テクスチャを生成
    pub fn process(
        &mut self,
        rgba_data: &[u8],
        width: u32,
        height: u32,
        timestamp: i64,
    ) -> Result<ID3D11Texture2D> {
        unsafe {
            // 解像度が変更された場合は再設定
            self.resize(width, height)?;

            // RGBA を D3D11 テクスチャにアップロード
            let rgba_texture = self.upload_rgba_to_texture(rgba_data, width, height)?;

            // BGRA テクスチャを作成
            let bgra_texture = self.create_bgra_texture(width, height)?;

            // GPU側でRGBA→BGRA変換を行う
            self.convert_rgba_to_bgra(&rgba_texture, &bgra_texture, width, height)?;

            let input_texture = bgra_texture;

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
