use anyhow::{Context, Result};
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;

use super::converter::ColorConverter;
use super::texture_pool::TexturePool;
use super::video_processor::VideoProcessor;
use crate::windows::utils::d3d::D3D11Resources;

/// Video Processor MFT Preprocessor (RGBA -> BGRA -> NV12 + Resize)
pub struct VideoProcessorPreprocessor {
    texture_pool: TexturePool,
    converter: ColorConverter,
    video_processor: VideoProcessor,
}

impl VideoProcessorPreprocessor {
    /// Create Video Processor MFT
    pub fn create(d3d_resources: D3D11Resources) -> Result<Self> {
        let video_processor = VideoProcessor::new()?;

        // Setup D3D Manager (using methods from video_processor if needed,
        // but previously it was d3d_resources.setup_mft(&transform))
        d3d_resources.setup_mft(video_processor.transform())?;

        Ok(Self {
            texture_pool: TexturePool::new(d3d_resources),
            converter: ColorConverter::new(),
            video_processor,
        })
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
        // 1. Reconfigure if needed
        if self
            .texture_pool
            .needs_reconfigure(src_width, src_height, dst_width, dst_height)
        {
            self.texture_pool.clear();
            self.video_processor
                .configure(src_width, src_height, dst_width, dst_height)
                .context("Failed to configure video processor in process")?;
        }

        // 2. Upload RGBA & Create target textures
        let rgba_texture = self
            .texture_pool
            .ensure_rgba_texture(rgba_data, src_width, src_height)?;
        let bgra_texture = self
            .texture_pool
            .ensure_bgra_texture(src_width, src_height)?;

        // 3. Convert RGBA -> BGRA
        {
            let device = self.texture_pool.device();
            self.converter.ensure_shader(&device)?;
        }

        // Get views from pool
        let srv = self.texture_pool.get_rgba_srv(&rgba_texture)?;
        let uav = self.texture_pool.get_bgra_uav(&bgra_texture)?;

        {
            let context = self.texture_pool.context();
            self.converter
                .convert(&context, &srv, &uav, src_width, src_height)?;
        }

        // 4. Process with MFT (BGRA -> NV12 + Resize)
        let output_texture_result = self.video_processor.process(bgra_texture, timestamp)?;

        // 5. 出力テクスチャを確保して返す
        // MFTから出力が得られなかった場合（NEED_MORE_INPUTなど）、
        // プールにある出力テクスチャ（前回のフレームまたは空）をフォールバックとして返す
        let pool_output_texture = self
            .texture_pool
            .ensure_output_texture(dst_width, dst_height)?;

        if let Some(tex) = output_texture_result {
            Ok(tex)
        } else {
            Ok(pool_output_texture)
        }
    }
}
