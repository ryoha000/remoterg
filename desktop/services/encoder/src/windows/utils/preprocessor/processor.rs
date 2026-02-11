use anyhow::{Context, Result};
use std::mem::ManuallyDrop;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11ShaderResourceView, ID3D11Texture2D, ID3D11UnorderedAccessView, D3D11_TEXTURE2D_DESC,
};
use windows::Win32::Media::MediaFoundation::{
    IMFDXGIBuffer, IMFTransform, MFCreateDXGISurfaceBuffer, MFCreateMediaType, MFCreateSample,
    MFMediaType_Video, MFVideoFormat_ARGB32, MFVideoFormat_NV12, MFVideoInterlace_Progressive,
    MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_OUTPUT_DATA_BUFFER,
    MF_E_TRANSFORM_NEED_MORE_INPUT, MF_E_TRANSFORM_STREAM_CHANGE,
};

use crate::windows::utils::d3d::D3D11Resources;
use super::converter::ColorConverter;
use super::texture;

/// Video Processor MFT Preprocessor (RGBA -> BGRA -> NV12 + Resize)
pub struct VideoProcessorPreprocessor {
    transform: IMFTransform,
    d3d_resources: D3D11Resources,
    rgba_texture: Option<ID3D11Texture2D>,
    bgra_texture: Option<ID3D11Texture2D>,
    output_texture: Option<ID3D11Texture2D>,
    rgba_srv: Option<ID3D11ShaderResourceView>,
    bgra_uav: Option<ID3D11UnorderedAccessView>,
    converter: ColorConverter,
}

impl VideoProcessorPreprocessor {
    /// Create Video Processor MFT
    pub fn create(d3d_resources: D3D11Resources) -> Result<Self> {
        unsafe {
            let transform = crate::windows::utils::mf::find_video_processor()
                .context("Failed to find Video Processor MFT")?;

            // Setup D3D Manager
            d3d_resources.setup_mft(&transform)?;

            let preprocessor = Self {
                transform,
                d3d_resources,
                rgba_texture: None,
                bgra_texture: None,
                output_texture: None,
                rgba_srv: None,
                bgra_uav: None,
                converter: ColorConverter::new(),
            };

            // Media types are set in process(), not here.

            Ok(preprocessor)
        }
    }

    /// Set media types
    fn setup_media_types(
        &mut self,
        src_width: u32,
        src_height: u32,
        dst_width: u32,
        dst_height: u32,
    ) -> Result<()> {
        unsafe {
            // Input Media Type (BGRA)
            let input_media_type = MFCreateMediaType()
                .context("Failed to create input media type")?;

            input_media_type
                .SetGUID(
                    &windows::Win32::Media::MediaFoundation::MF_MT_MAJOR_TYPE,
                    &MFMediaType_Video,
                )
                .context("Failed to set input major type")?;

            input_media_type
                .SetGUID(
                    &windows::Win32::Media::MediaFoundation::MF_MT_SUBTYPE,
                    &MFVideoFormat_ARGB32,
                )
                .context("Failed to set input subtype")?;

            let frame_size = ((src_width as u64) << 32) | (src_height as u64);
            input_media_type
                .SetUINT64(
                    &windows::Win32::Media::MediaFoundation::MF_MT_FRAME_SIZE,
                    frame_size,
                )
                .context("Failed to set input frame size")?;

            let frame_rate = (60u64 << 32) | 1u64;
            input_media_type
                .SetUINT64(
                    &windows::Win32::Media::MediaFoundation::MF_MT_FRAME_RATE,
                    frame_rate,
                )
                .context("Failed to set input frame rate")?;

            input_media_type
                .SetUINT32(
                    &windows::Win32::Media::MediaFoundation::MF_MT_INTERLACE_MODE,
                    MFVideoInterlace_Progressive.0 as u32,
                )
                .context("Failed to set input interlace mode")?;

            self.transform
                .SetInputType(0, &input_media_type, 0)
                .context("Failed to set Video Processor input type")?;

            // Output Media Type (NV12)
            let output_media_type = MFCreateMediaType()
                .context("Failed to create output media type")?;

            output_media_type
                .SetGUID(
                    &windows::Win32::Media::MediaFoundation::MF_MT_MAJOR_TYPE,
                    &MFMediaType_Video,
                )
                .context("Failed to set output major type")?;

            output_media_type
                .SetGUID(
                    &windows::Win32::Media::MediaFoundation::MF_MT_SUBTYPE,
                    &MFVideoFormat_NV12,
                )
                .context("Failed to set output subtype")?;

            let output_frame_size = ((dst_width as u64) << 32) | (dst_height as u64);
            output_media_type
                .SetUINT64(
                    &windows::Win32::Media::MediaFoundation::MF_MT_FRAME_SIZE,
                    output_frame_size,
                )
                .context("Failed to set output frame size")?;

            output_media_type
                .SetUINT64(
                    &windows::Win32::Media::MediaFoundation::MF_MT_FRAME_RATE,
                    frame_rate,
                )
                .context("Failed to set output frame rate")?;

            output_media_type
                .SetUINT32(
                    &windows::Win32::Media::MediaFoundation::MF_MT_INTERLACE_MODE,
                    MFVideoInterlace_Progressive.0 as u32,
                )
                .context("Failed to set output interlace mode")?;

            self.transform
                .SetOutputType(0, &output_media_type, 0)
                .context("Failed to set Video Processor output type")?;

            // Notify start of stream (required for async MFT)
            self.transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
                .context("Failed to notify begin streaming")?;

            self.transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
                .context("Failed to notify start of stream")?;

            Ok(())
        }
    }

    /// Upload RGBA data to D3D11 texture
    fn ensure_rgba_texture(
        &mut self,
        rgba_data: &[u8],
        width: u32,
        height: u32,
    ) -> Result<ID3D11Texture2D> {
        let needs_recreate = self.rgba_texture.is_none() || unsafe {
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            self.rgba_texture.as_ref().unwrap().GetDesc(&mut desc);
            desc.Width != width || desc.Height != height
        };

        if needs_recreate {
            self.rgba_texture = Some(texture::create_rgba_texture(&self.d3d_resources.device, width, height)?);
            // Invalidate dependent views
            self.rgba_srv = None;
        }

        let texture = self.rgba_texture.as_ref().unwrap();
        texture::upload_rgba_data(&self.d3d_resources.context, texture, rgba_data, width, height);

        Ok(texture.clone())
    }

    /// Create BGRA texture (conversion target)
    fn ensure_bgra_texture(&mut self, width: u32, height: u32) -> Result<ID3D11Texture2D> {
        let needs_recreate = self.bgra_texture.is_none() || unsafe {
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            self.bgra_texture.as_ref().unwrap().GetDesc(&mut desc);
            desc.Width != width || desc.Height != height
        };

        if needs_recreate {
            self.bgra_texture = Some(texture::create_bgra_texture(&self.d3d_resources.device, width, height)?);
            // Invalidate dependent views
            self.bgra_uav = None;
        }

        Ok(self.bgra_texture.as_ref().unwrap().clone())
    }

    /// Execute RGBA -> BGRA conversion on GPU
    fn convert_rgba_to_bgra(
        &mut self,
        rgba_texture: &ID3D11Texture2D,
        bgra_texture: &ID3D11Texture2D,
        width: u32,
        height: u32,
    ) -> Result<()> {
        self.converter.ensure_shader(&self.d3d_resources.device)?;

        unsafe {
            if self.rgba_srv.is_none() {
                let mut srv: Option<ID3D11ShaderResourceView> = None;
                self.d3d_resources
                    .device
                    .CreateShaderResourceView(rgba_texture, None, Some(&mut srv))
                    .context("Failed to create RGBA SRV")?;
                self.rgba_srv = srv;
            }

            if self.bgra_uav.is_none() {
                let mut uav: Option<ID3D11UnorderedAccessView> = None;
                self.d3d_resources
                    .device
                    .CreateUnorderedAccessView(bgra_texture, None, Some(&mut uav))
                    .context("Failed to create BGRA UAV")?;
                self.bgra_uav = uav;
            }

            self.converter.convert(
                &self.d3d_resources.context,
                self.rgba_srv.as_ref().unwrap(),
                self.bgra_uav.as_ref().unwrap(),
                width,
                height,
            )?;
        }
        Ok(())
    }

    /// Create NV12 output texture
    fn ensure_output_texture(&mut self, width: u32, height: u32) -> Result<ID3D11Texture2D> {
        let needs_recreate = self.output_texture.is_none() || unsafe {
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            self.output_texture.as_ref().unwrap().GetDesc(&mut desc);
            desc.Width != width || desc.Height != height
        };

        if needs_recreate {
            self.output_texture = Some(texture::create_nv12_texture(&self.d3d_resources.device, width, height)?);
        }

        Ok(self.output_texture.as_ref().unwrap().clone())
    }

    /// Process RGBA data and generate NV12 texture
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
            // Reconfigure if needed
            let needs_reconfigure = self.output_texture.is_none()
                || {
                    let mut desc = D3D11_TEXTURE2D_DESC::default();
                    if let Some(tex) = &self.output_texture {
                        tex.GetDesc(&mut desc);
                        desc.Width != dst_width || desc.Height != dst_height
                    } else {
                        true
                    }
                }
                || {
                    let mut desc = D3D11_TEXTURE2D_DESC::default();
                    if let Some(tex) = &self.rgba_texture {
                        tex.GetDesc(&mut desc);
                        desc.Width != src_width || desc.Height != src_height
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
                self.setup_media_types(src_width, src_height, dst_width, dst_height)
                    .context("Failed to setup media types in process")?;
            }

            // Upload RGBA
            let rgba_texture = self.ensure_rgba_texture(rgba_data, src_width, src_height)?;

            // Create BGRA
            let bgra_texture = self.ensure_bgra_texture(src_width, src_height)?;

            // Convert RGBA -> BGRA
            self.convert_rgba_to_bgra(&rgba_texture, &bgra_texture, src_width, src_height)?;

            let input_texture = bgra_texture;

            // Create Output Texture
            let output_texture = self.ensure_output_texture(dst_width, dst_height)?;

            // Create DXGI Surface Buffer
            let input_buffer =
                MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, &input_texture, 0, false)
                    .map_err(|e| {
                        anyhow::anyhow!(
                            "Failed to create DXGI surface buffer: {}",
                            e
                        )
                    })?;

            // Create Input Sample
            let input_sample = MFCreateSample()
                .context("Failed to create input sample")?;

            input_sample
                .AddBuffer(&input_buffer)
                .context("Failed to add buffer to sample")?;

            input_sample
                .SetSampleTime(timestamp)
                .context("Failed to set sample time")?;

            // ProcessInput
            self.transform
                .ProcessInput(0, &input_sample, 0)
                .context("Failed to process input in Video Processor")?;

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
                                .context("Failed to get output buffer")?;

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

            Ok(output_texture_result.unwrap_or(output_texture))
        }
    }
}
