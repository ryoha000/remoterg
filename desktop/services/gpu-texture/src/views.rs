use anyhow::{Context, Result};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11RenderTargetView, ID3D11ShaderResourceView, ID3D11Texture2D, ID3D11UnorderedAccessView,
};

use crate::D3D11Device;

/// Shader Resource Viewを作成
pub fn create_shader_resource_view(
    device: &D3D11Device,
    texture: &ID3D11Texture2D,
) -> Result<ID3D11ShaderResourceView> {
    let mut srv: Option<ID3D11ShaderResourceView> = None;
    unsafe {
        device
            .device()
            .CreateShaderResourceView(texture, None, Some(&mut srv))
            .context("Failed to create shader resource view")?;
    }
    srv.ok_or_else(|| anyhow::anyhow!("SRV creation returned None"))
}

/// Unordered Access Viewを作成
pub fn create_unordered_access_view(
    device: &D3D11Device,
    texture: &ID3D11Texture2D,
) -> Result<ID3D11UnorderedAccessView> {
    let mut uav: Option<ID3D11UnorderedAccessView> = None;
    unsafe {
        device
            .device()
            .CreateUnorderedAccessView(texture, None, Some(&mut uav))
            .context("Failed to create unordered access view")?;
    }
    uav.ok_or_else(|| anyhow::anyhow!("UAV creation returned None"))
}

/// Render Target Viewを作成
pub fn create_render_target_view(
    device: &D3D11Device,
    texture: &ID3D11Texture2D,
) -> Result<ID3D11RenderTargetView> {
    let mut rtv: Option<ID3D11RenderTargetView> = None;
    unsafe {
        device
            .device()
            .CreateRenderTargetView(texture, None, Some(&mut rtv))
            .context("Failed to create render target view")?;
    }
    rtv.ok_or_else(|| anyhow::anyhow!("RTV creation returned None"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TextureBuilder;

    #[test]
    fn test_create_shader_resource_view() {
        let device = D3D11Device::new().expect("Failed to create device");
        let texture = TextureBuilder::rgba_input()(64, 64)
            .build(&device)
            .expect("Failed to create texture");

        let srv = create_shader_resource_view(&device, &texture);
        assert!(srv.is_ok());
    }

    #[test]
    fn test_create_unordered_access_view() {
        let device = D3D11Device::new().expect("Failed to create device");
        let texture = TextureBuilder::rgba_input()(64, 64)
            .build(&device)
            .expect("Failed to create texture");

        let uav = create_unordered_access_view(&device, &texture);
        assert!(uav.is_ok());
    }

    #[test]
    fn test_create_render_target_view() {
        let device = D3D11Device::new().expect("Failed to create device");
        let texture = TextureBuilder::bgra_target()(64, 64)
            .build(&device)
            .expect("Failed to create texture");

        let rtv = create_render_target_view(&device, &texture);
        assert!(rtv.is_ok());
    }
}
