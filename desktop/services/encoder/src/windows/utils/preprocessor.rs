use anyhow::{Context, Result};
use std::mem::ManuallyDrop;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_UNORDERED_ACCESS, D3D11_BIND_VIDEO_ENCODER, ID3D11ComputeShader, ID3D11ShaderResourceView, ID3D11UnorderedAccessView
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

use super::d3d::D3D11Resources;
use super::mf;

/// Video Processor MFT による前処理（RGBA → BGRA → NV12 + リサイズ）
pub struct VideoProcessorPreprocessor {
    transform: IMFTransform,
    d3d_resources: D3D11Resources,
    rgba_texture: Option<ID3D11Texture2D>,
    bgra_texture: Option<ID3D11Texture2D>,
    output_texture: Option<ID3D11Texture2D>,
    rgba_srv: Option<ID3D11ShaderResourceView>,
    bgra_uav: Option<ID3D11UnorderedAccessView>,
    compute_shader: Option<ID3D11ComputeShader>,
    // 現在設定されているアライメント後の解像度を保持
    current_aligned_width: u32,
    current_aligned_height: u32,
}

impl VideoProcessorPreprocessor {
    /// Video Processor MFT を作成
    pub fn create(d3d_resources: D3D11Resources) -> Result<Self> {
        unsafe {
            let transform = mf::find_video_processor()
                .context("Failed to find Video Processor MFT")?;

            d3d_resources.setup_mft(&transform)?;

            Ok(Self {
                transform,
                d3d_resources,
                rgba_texture: None,
                bgra_texture: None,
                output_texture: None,
                rgba_srv: None,
                bgra_uav: None,
                compute_shader: None,
                current_aligned_width: 0,
                current_aligned_height: 0,
            })
        }
    }

    fn setup_media_types(&mut self, width: u32, height: u32) -> Result<()> {
        unsafe {
            // 入力メディアタイプ（BGRA - アライメント後のサイズ）
            let input_media_type = MFCreateMediaType().context("Failed to create input type")?;
            input_media_type.SetGUID(&windows::Win32::Media::MediaFoundation::MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            input_media_type.SetGUID(&windows::Win32::Media::MediaFoundation::MF_MT_SUBTYPE, &MFVideoFormat_ARGB32)?;
            
            let frame_size = ((width as u64) << 32) | (height as u64);
            input_media_type.SetUINT64(&windows::Win32::Media::MediaFoundation::MF_MT_FRAME_SIZE, frame_size)?;
            input_media_type.SetUINT64(&windows::Win32::Media::MediaFoundation::MF_MT_FRAME_RATE, (60u64 << 32) | 1u64)?;
            input_media_type.SetUINT32(&windows::Win32::Media::MediaFoundation::MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;

            self.transform.SetInputType(0, &input_media_type, 0).context("Failed to set VP input type")?;

            // 出力メディアタイプ（NV12 - アライメント後のサイズ）
            let output_media_type = MFCreateMediaType().context("Failed to create output type")?;
            output_media_type.SetGUID(&windows::Win32::Media::MediaFoundation::MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            output_media_type.SetGUID(&windows::Win32::Media::MediaFoundation::MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
            output_media_type.SetUINT64(&windows::Win32::Media::MediaFoundation::MF_MT_FRAME_SIZE, frame_size)?;
            output_media_type.SetUINT64(&windows::Win32::Media::MediaFoundation::MF_MT_FRAME_RATE, (60u64 << 32) | 1u64)?;
            output_media_type.SetUINT32(&windows::Win32::Media::MediaFoundation::MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;

            self.transform.SetOutputType(0, &output_media_type, 0).context("Failed to set VP output type")?;

            self.transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
            self.transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;

            self.current_aligned_width = width;
            self.current_aligned_height = height;
            Ok(())
        }
    }



    /// Compute Shaderを作成
    fn create_compute_shader(&mut self) -> Result<()> {
        if self.compute_shader.is_some() { return Ok(()); }
        unsafe {
            let shader_code = r#"
                Texture2D<float4> rgba_texture : register(t0);
                RWTexture2D<float4> bgra_texture : register(u0);
                [numthreads(8, 8, 1)]
                void CSMain(uint3 id : SV_DispatchThreadID) {
                    // 入力が大きくても、出力テクスチャのサイズ内でのみ書き込む
                    float4 rgba = rgba_texture[id.xy];
                    bgra_texture[id.xy] = rgba;
                }
            "#;

            use windows::core::PCSTR;
            use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;

            let mut compiled_shader = None;
            D3DCompile(shader_code.as_ptr() as _, shader_code.len(), None, None, None, 
                       PCSTR(b"CSMain\0".as_ptr()), PCSTR(b"cs_5_0\0".as_ptr()), 1 << 15, 0, 
                       &mut compiled_shader, None)?;

            let compiled_shader = compiled_shader.unwrap();
            let mut shader = None;
            self.d3d_resources.device.CreateComputeShader(
                std::slice::from_raw_parts(compiled_shader.GetBufferPointer() as *const u8, compiled_shader.GetBufferSize()),
                None, Some(&mut shader))?;
            self.compute_shader = shader;
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
            // RGBA = 4 bytes per pixel
            // width は src_width なので、入力画像のストライドと一致するはず
            let row_pitch = width * 4;
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
                    BindFlags: (
                        // D3D11_BIND_VIDEO_ENCODER.0
                        D3D11_BIND_SHADER_RESOURCE.0
                        | D3D11_BIND_RENDER_TARGET.0
                        | D3D11_BIND_UNORDERED_ACCESS.0
                    ) as u32,
                    CPUAccessFlags: 0,
                    MiscFlags: 0,
                };

                let mut texture: Option<ID3D11Texture2D> = None;
                self.d3d_resources
                    .device
                    .CreateTexture2D(&desc, None, Some(&mut texture))?;

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
                    .CreateTexture2D(&desc, None, Some(&mut texture))?;

                self.output_texture = texture;
            }

            Ok(self.output_texture.as_ref().unwrap().clone())
        }
    }

    /// RGBA データを処理して NV12 テクスチャを生成
    pub fn process(
        &mut self,
        rgba_data: &[u8],
        src_width: u32,
        src_height: u32,
        dst_width: u32,
        dst_height: u32,
        timestamp: i64,
    ) -> Result<ID3D11Texture2D> {
        unsafe {
            // 必要に応じてリソースを再確保・メディアタイプ再設定
            // 簡易的に、既存のテクスチャサイズと異なれば再設定とする
             let needs_reconfigure = self.output_texture.is_none() || {
                let mut desc = D3D11_TEXTURE2D_DESC::default();
                if let Some(tex) = &self.output_texture {
                    tex.GetDesc(&mut desc);
                    desc.Width != dst_width || desc.Height != dst_height
                } else {
                    true
                }
            };

            if needs_reconfigure {
                self.rgba_texture = None;
                self.bgra_texture = None;
                self.output_texture = None;
                self.rgba_srv = None;
                self.bgra_uav = None;
                self.setup_media_types(dst_width, dst_height)
                    .context("Failed to setup media types in process")?;
            }

            // 2. RGBAアップロード（生サイズ 1923x1121）
            if self.rgba_texture.is_none() {
                let desc = D3D11_TEXTURE2D_DESC {
                    Width: src_width, Height: src_height, MipLevels: 1, ArraySize: 1,
                    Format: DXGI_FORMAT_R8G8B8A8_UNORM, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                    Usage: D3D11_USAGE_DEFAULT, BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32, ..Default::default()
                };
                let mut tex = None;
                self.d3d_resources.device.CreateTexture2D(&desc, None, Some(&mut tex))?;
                self.rgba_texture = tex;
            }
            let rgba_tex = self.rgba_texture.as_ref().unwrap().clone();
            self.d3d_resources.context.UpdateSubresource(&rgba_tex, 0, None, rgba_data.as_ptr() as _, src_width * 4, 0);

            // 3. BGRA変換用テクスチャ（アライメント済サイズ 1920x1120）
            if self.bgra_texture.is_none() {
                let desc = D3D11_TEXTURE2D_DESC {
                    Width: dst_width, Height: dst_height, MipLevels: 1, ArraySize: 1,
                    Format: DXGI_FORMAT_B8G8R8A8_UNORM, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                    Usage: D3D11_USAGE_DEFAULT, 
                    BindFlags: (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_UNORDERED_ACCESS.0 | D3D11_BIND_RENDER_TARGET.0) as u32, 
                    ..Default::default()
                };
                let mut tex = None;
                self.d3d_resources.device.CreateTexture2D(&desc, None, Some(&mut tex))?;
                self.bgra_texture = tex;
            }
            let bgra_tex = self.bgra_texture.as_ref().unwrap().clone();

            // 4. Compute Shader実行（1923x1121 -> 1920x1120）
            self.create_compute_shader()?;
            if self.rgba_srv.is_none() {
                let mut srv = None;
                self.d3d_resources.device.CreateShaderResourceView(&rgba_tex, None, Some(&mut srv))?;
                self.rgba_srv = srv;
            }
            if self.bgra_uav.is_none() {
                let mut uav = None;
                self.d3d_resources.device.CreateUnorderedAccessView(&bgra_tex, None, Some(&mut uav))?;
                self.bgra_uav = uav;
            }

            self.d3d_resources.context.CSSetShader(self.compute_shader.as_ref(), None);
            self.d3d_resources.context.CSSetShaderResources(0, Some(&[self.rgba_srv.clone()]));
            self.d3d_resources.context.CSSetUnorderedAccessViews(0, 1, Some(&self.bgra_uav), None);
            // 実行範囲はアライメント後のサイズ
            self.d3d_resources.context.Dispatch((dst_width + 7) / 8, (dst_height + 7) / 8, 1);
            
            // クリーンアップ
            self.d3d_resources.context.CSSetShader(None, None);
            self.d3d_resources.context.CSSetShaderResources(0, Some(&[None]));
            self.d3d_resources.context.CSSetUnorderedAccessViews(0, 1, Some(&None), None);

            // 5. Output用NV12テクスチャ（1920x1120）
            if self.output_texture.is_none() {
                let desc = D3D11_TEXTURE2D_DESC {
                    Width: dst_width, Height: dst_height, MipLevels: 1, ArraySize: 1,
                    Format: DXGI_FORMAT_NV12, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                    Usage: D3D11_USAGE_DEFAULT, 
                    BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32, ..Default::default()
                };
                let mut tex = None;
                self.d3d_resources.device.CreateTexture2D(&desc, None, Some(&mut tex))?;
                self.output_texture = tex;
            }
            let output_tex = self.output_texture.as_ref().unwrap().clone();

            // 6. MFT Process
            let input_buffer = MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, &bgra_tex, 0, false)?;
            let input_sample = MFCreateSample()?;
            input_sample.AddBuffer(&input_buffer)?;
            input_sample.SetSampleTime(timestamp)?;

            self.transform.ProcessInput(0, &input_sample, 0)?;

            // ProcessOutput で NV12 テクスチャに書き込ませる
            // 出力用バッファとサンプルを作成
            let output_buffer =
                MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, &output_tex, 0, false)
                    .map_err(|e| {
                        anyhow::anyhow!(
                        "Failed to create DXGI surface buffer for output (format=NV12, width={}, height={}): {}",
                        dst_width,
                        dst_height,
                        e
                    )
                    })?;

            let output_sample = MFCreateSample()
                .ok()
                .context("Failed to create output sample")?;

            output_sample
                .AddBuffer(&output_buffer)
                .ok()
                .context("Failed to add buffer to output sample")?;

            // 非同期MFTの場合、ProcessOutputをループで呼び出して
            // MF_E_TRANSFORM_NEED_MORE_INPUTが返されるまで繰り返す
            // ただし、出力サンプルを渡す場合は1回で済むことが多い
            let mut output_produced = false;

            loop {
                // 出力サンプルを渡す
                let mut output_data_buffer = MFT_OUTPUT_DATA_BUFFER {
                    dwStreamID: 0,
                    pSample: ManuallyDrop::new(Some(output_sample.clone())),
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
                        output_produced = true;
                        // 成功したら完了（1フレーム分）
                        break;
                    }
                    Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => {
                        // 入力が足りない（まだ出力できない）
                        break;
                    }
                    Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                        // ストリーム変更が発生した場合は警告を出して続行
                        tracing::warn!("Video Processor: stream change detected");
                        // ストリーム変更の場合は再試行が必要かもしれないが、
                        // ここでは一旦ループを抜ける（次のフレームで再設定されるはず）
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

            if !output_produced {
                tracing::warn!("Video Processor: ProcessOutput did not produce output for timestamp {}", timestamp);
            }

            // Video Processor MFTが書き込んだ（はずの）テクスチャを返す
            Ok(output_tex)


        }
    }
}
