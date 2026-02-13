use crate::device::D3D11Device;
use anyhow::{Context, Result};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Texture2D, D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_CPU_ACCESS_READ,
    D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_RESOURCE_MISC_SHARED, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};

/// Shared texture を表す構造体
pub struct SharedTexture {
    texture: ID3D11Texture2D,
    handle: u64,
    pub width: u32,
    pub height: u32,
}

impl SharedTexture {
    /// RGBA データから shared texture を作成
    pub fn from_rgba(device: &D3D11Device, rgba: &[u8], width: u32, height: u32) -> Result<Self> {
        let expected_size = (width * height * 4) as usize;
        anyhow::ensure!(
            rgba.len() >= expected_size,
            "RGBA data size mismatch: expected {}, got {}",
            expected_size,
            rgba.len()
        );

        unsafe {
            // RGBA → BGRA 変換
            let mut bgra_data = Vec::with_capacity(rgba.len());
            for chunk in rgba.chunks_exact(4) {
                bgra_data.push(chunk[2]); // B
                bgra_data.push(chunk[1]); // G
                bgra_data.push(chunk[0]); // R
                bgra_data.push(chunk[3]); // A
            }

            // Texture descriptor を作成
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
                MiscFlags: D3D11_RESOURCE_MISC_SHARED.0 as u32,
            };

            // まず空のTextureを作成（video-captureと同じパターン）
            let mut texture: Option<ID3D11Texture2D> = None;
            device
                .device()
                .CreateTexture2D(&desc, None, Some(&mut texture)) // 初期データなし
                .context("Failed to create texture")?;
            let texture = texture.context("Texture is None")?;

            // UpdateSubresourceでデータを書き込む
            device.context().UpdateSubresource(
                &texture,
                0,
                None,
                bgra_data.as_ptr() as *const _,
                width * 4,
                0,
            );

            // Shared handle を取得
            let handle = Self::get_shared_handle(&texture)?;

            Ok(Self {
                texture,
                handle,
                width,
                height,
            })
        }
    }

    /// Shared handle から texture を開く
    pub fn from_handle(device: &D3D11Device, handle: u64) -> Result<Self> {
        unsafe {
            let handle_ptr = HANDLE(handle as usize as *mut std::ffi::c_void);
            let mut texture: Option<ID3D11Texture2D> = None;

            device
                .device()
                .OpenSharedResource(handle_ptr, &mut texture)
                .context("Failed to open shared resource")?;

            let texture = texture.context("Shared texture is None")?;

            // Texture の情報を取得
            let mut desc = std::mem::zeroed();
            texture.GetDesc(&mut desc);

            Ok(Self {
                texture,
                handle,
                width: desc.Width,
                height: desc.Height,
            })
        }
    }

    /// Shared handle を取得
    fn get_shared_handle(texture: &ID3D11Texture2D) -> Result<u64> {
        unsafe {
            use windows::core::Interface;
            use windows::Win32::Graphics::Dxgi::IDXGIResource;

            let resource: IDXGIResource =
                texture.cast().context("Failed to cast to IDXGIResource")?;
            let handle = resource
                .GetSharedHandle()
                .context("Failed to get shared handle")?;

            Ok(handle.0 as usize as u64)
        }
    }

    /// Shared handle を取得
    pub fn handle(&self) -> u64 {
        self.handle
    }

    /// Texture から RGBA データを読み取る
    pub fn to_rgba(&self, device: &D3D11Device) -> Result<Vec<u8>> {
        unsafe {
            // Staging texture を作成
            let staging_desc = D3D11_TEXTURE2D_DESC {
                Width: self.width,
                Height: self.height,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                MiscFlags: 0,
            };

            let mut staging_texture: Option<ID3D11Texture2D> = None;
            device
                .device()
                .CreateTexture2D(&staging_desc, None, Some(&mut staging_texture))
                .context("Failed to create staging texture")?;
            let staging_texture = staging_texture.context("Staging texture is None")?;

            // Texture をコピー
            device
                .context()
                .CopyResource(&staging_texture, &self.texture);

            // Staging texture をマップ
            let mut mapped: D3D11_MAPPED_SUBRESOURCE = std::mem::zeroed();
            device
                .context()
                .Map(&staging_texture, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .context("Failed to map staging texture")?;

            // BGRA → RGBA 変換しながらデータをコピー
            let row_pitch = mapped.RowPitch as usize;
            let width_usize = self.width as usize;
            let height_usize = self.height as usize;
            let row_size = width_usize * 4;
            let mut rgba_data = Vec::with_capacity(row_size * height_usize);

            let src_ptr = mapped.pData as *const u8;
            for y in 0..height_usize {
                let src_row = std::slice::from_raw_parts(src_ptr.add(y * row_pitch), row_size);
                for x in 0..width_usize {
                    let pixel_offset = x * 4;
                    let b = src_row[pixel_offset];
                    let g = src_row[pixel_offset + 1];
                    let r = src_row[pixel_offset + 2];
                    let a = src_row[pixel_offset + 3];
                    // BGRA → RGBA
                    rgba_data.push(r);
                    rgba_data.push(g);
                    rgba_data.push(b);
                    rgba_data.push(a);
                }
            }

            device.context().Unmap(&staging_texture, 0);

            Ok(rgba_data)
        }
    }
}
