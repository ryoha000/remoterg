use anyhow::{Context, Result};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Texture2D, D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE,
    D3D11_BIND_UNORDERED_ACCESS, D3D11_CPU_ACCESS_READ, D3D11_RESOURCE_MISC_SHARED,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE, D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_NV12, DXGI_FORMAT_R8G8B8A8_UNORM,
    DXGI_SAMPLE_DESC,
};

use crate::D3D11Device;

/// D3D11テクスチャを柔軟に作成するためのBuilder
pub struct TextureBuilder {
    width: u32,
    height: u32,
    format: DXGI_FORMAT,
    bind_flags: u32,
    misc_flags: u32,
    usage: D3D11_USAGE,
    cpu_access_flags: u32,
}

impl TextureBuilder {
    /// 新しいTextureBuilderを作成
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            format: DXGI_FORMAT_R8G8B8A8_UNORM,
            bind_flags: 0,
            misc_flags: 0,
            usage: D3D11_USAGE_DEFAULT,
            cpu_access_flags: 0,
        }
    }

    /// RGBAインプット用のプリセット（SRV + UAV）
    pub fn rgba_input() -> impl Fn(u32, u32) -> Self {
        |width, height| Self {
            width,
            height,
            format: DXGI_FORMAT_R8G8B8A8_UNORM,
            bind_flags: (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_UNORDERED_ACCESS.0) as u32,
            misc_flags: 0,
            usage: D3D11_USAGE_DEFAULT,
            cpu_access_flags: 0,
        }
    }

    /// BGRAターゲット用のプリセット（SRV + RTV + UAV）
    pub fn bgra_target() -> impl Fn(u32, u32) -> Self {
        |width, height| Self {
            width,
            height,
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            bind_flags: (D3D11_BIND_SHADER_RESOURCE.0
                | D3D11_BIND_RENDER_TARGET.0
                | D3D11_BIND_UNORDERED_ACCESS.0) as u32,
            misc_flags: 0,
            usage: D3D11_USAGE_DEFAULT,
            cpu_access_flags: 0,
        }
    }

    /// NV12出力用のプリセット（RTV + SRV）
    pub fn nv12_output() -> impl Fn(u32, u32) -> Self {
        |width, height| Self {
            width,
            height,
            format: DXGI_FORMAT_NV12,
            bind_flags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
            misc_flags: 0,
            usage: D3D11_USAGE_DEFAULT,
            cpu_access_flags: 0,
        }
    }

    /// フォーマットを設定
    pub fn format(mut self, format: DXGI_FORMAT) -> Self {
        self.format = format;
        self
    }

    /// Shared textureフラグを追加
    pub fn shared(mut self) -> Self {
        self.misc_flags |= D3D11_RESOURCE_MISC_SHARED.0 as u32;
        self
    }

    /// Staging texture（CPU読み取り用）に設定
    pub fn staging(mut self) -> Self {
        self.usage = D3D11_USAGE_STAGING;
        self.bind_flags = 0;
        self.cpu_access_flags = D3D11_CPU_ACCESS_READ.0 as u32;
        self
    }

    /// Shader Resource bindフラグを追加
    pub fn bind_shader_resource(mut self) -> Self {
        self.bind_flags |= D3D11_BIND_SHADER_RESOURCE.0 as u32;
        self
    }

    /// Render Target bindフラグを追加
    pub fn bind_render_target(mut self) -> Self {
        self.bind_flags |= D3D11_BIND_RENDER_TARGET.0 as u32;
        self
    }

    /// Unordered Access bindフラグを追加
    pub fn bind_unordered_access(mut self) -> Self {
        self.bind_flags |= D3D11_BIND_UNORDERED_ACCESS.0 as u32;
        self
    }

    /// テクスチャを作成
    pub fn build(self, device: &D3D11Device) -> Result<ID3D11Texture2D> {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: self.width,
            Height: self.height,
            MipLevels: 1,
            ArraySize: 1,
            Format: self.format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: self.usage,
            BindFlags: self.bind_flags,
            CPUAccessFlags: self.cpu_access_flags,
            MiscFlags: self.misc_flags,
        };

        let mut texture: Option<ID3D11Texture2D> = None;
        unsafe {
            device
                .device()
                .CreateTexture2D(&desc, None, Some(&mut texture))
                .context("Failed to create texture")?;
        }

        texture.ok_or_else(|| anyhow::anyhow!("Texture creation returned None"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rgba_input_preset() {
        let device = D3D11Device::new().expect("Failed to create device");
        let texture = TextureBuilder::rgba_input()(64, 64)
            .build(&device)
            .expect("Failed to create RGBA input texture");

        unsafe {
            let mut desc = std::mem::zeroed();
            texture.GetDesc(&mut desc);
            assert_eq!(desc.Width, 64);
            assert_eq!(desc.Height, 64);
            assert_eq!(desc.Format, DXGI_FORMAT_R8G8B8A8_UNORM);
        }
    }

    #[test]
    fn test_bgra_target_preset() {
        let device = D3D11Device::new().expect("Failed to create device");
        let texture = TextureBuilder::bgra_target()(128, 128)
            .build(&device)
            .expect("Failed to create BGRA target texture");

        unsafe {
            let mut desc = std::mem::zeroed();
            texture.GetDesc(&mut desc);
            assert_eq!(desc.Width, 128);
            assert_eq!(desc.Height, 128);
            assert_eq!(desc.Format, DXGI_FORMAT_B8G8R8A8_UNORM);
        }
    }

    #[test]
    fn test_nv12_output_preset() {
        let device = D3D11Device::new().expect("Failed to create device");
        let texture = TextureBuilder::nv12_output()(256, 256)
            .build(&device)
            .expect("Failed to create NV12 output texture");

        unsafe {
            let mut desc = std::mem::zeroed();
            texture.GetDesc(&mut desc);
            assert_eq!(desc.Width, 256);
            assert_eq!(desc.Height, 256);
            assert_eq!(desc.Format, DXGI_FORMAT_NV12);
        }
    }

    #[test]
    fn test_shared_texture() {
        let device = D3D11Device::new().expect("Failed to create device");
        let texture = TextureBuilder::new(64, 64)
            .shared()
            .bind_shader_resource()
            .build(&device)
            .expect("Failed to create shared texture");

        unsafe {
            let mut desc = std::mem::zeroed();
            texture.GetDesc(&mut desc);
            assert!(desc.MiscFlags & D3D11_RESOURCE_MISC_SHARED.0 as u32 != 0);
        }
    }

    #[test]
    fn test_staging_texture() {
        let device = D3D11Device::new().expect("Failed to create device");
        let texture = TextureBuilder::new(64, 64)
            .staging()
            .build(&device)
            .expect("Failed to create staging texture");

        unsafe {
            let mut desc = std::mem::zeroed();
            texture.GetDesc(&mut desc);
            assert_eq!(desc.Usage, D3D11_USAGE_STAGING);
            assert!(desc.CPUAccessFlags & D3D11_CPU_ACCESS_READ.0 as u32 != 0);
        }
    }
}
