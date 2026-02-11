use anyhow::Result;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device, ID3D11DeviceContext, ID3D11ShaderResourceView, ID3D11Texture2D,
    ID3D11UnorderedAccessView, D3D11_TEXTURE2D_DESC,
};

use crate::windows::utils::d3d::D3D11Resources;
use super::texture;

/// プリプロセッサ用のD3D11テクスチャを管理
pub struct TexturePool {
    d3d_resources: D3D11Resources,
    rgba_texture: Option<ID3D11Texture2D>,
    bgra_texture: Option<ID3D11Texture2D>,
    output_texture: Option<ID3D11Texture2D>,
    rgba_srv: Option<ID3D11ShaderResourceView>,
    bgra_uav: Option<ID3D11UnorderedAccessView>,
}

impl TexturePool {
    pub fn new(d3d_resources: D3D11Resources) -> Self {
        Self {
            d3d_resources,
            rgba_texture: None,
            bgra_texture: None,
            output_texture: None,
            rgba_srv: None,
            bgra_uav: None,
        }
    }

    /// RGBAデータをD3D11テクスチャにアップロード
    pub fn ensure_rgba_texture(
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
            // 依存するViewを無効化
            self.rgba_srv = None;
        }

        let texture = self.rgba_texture.as_ref().unwrap();
        texture::upload_rgba_data(&self.d3d_resources.context, texture, rgba_data, width, height);

        Ok(texture.clone())
    }

    /// BGRAテクスチャを作成（変換先）
    pub fn ensure_bgra_texture(&mut self, width: u32, height: u32) -> Result<ID3D11Texture2D> {
        let needs_recreate = self.bgra_texture.is_none() || unsafe {
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            self.bgra_texture.as_ref().unwrap().GetDesc(&mut desc);
            desc.Width != width || desc.Height != height
        };

        if needs_recreate {
            self.bgra_texture = Some(texture::create_bgra_texture(&self.d3d_resources.device, width, height)?);
            // 依存するViewを無効化
            self.bgra_uav = None;
        }

        Ok(self.bgra_texture.as_ref().unwrap().clone())
    }

    /// NV12出力テクスチャを作成
    pub fn ensure_output_texture(&mut self, width: u32, height: u32) -> Result<ID3D11Texture2D> {
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

    pub fn get_rgba_srv(&mut self, texture: &ID3D11Texture2D) -> Result<ID3D11ShaderResourceView> {
        if self.rgba_srv.is_none() {
            let mut srv: Option<ID3D11ShaderResourceView> = None;
            unsafe {
                self.d3d_resources
                    .device
                    .CreateShaderResourceView(texture, None, Some(&mut srv))?;
            }
            self.rgba_srv = srv;
        }
        Ok(self.rgba_srv.as_ref().unwrap().clone())
    }

    pub fn get_bgra_uav(&mut self, texture: &ID3D11Texture2D) -> Result<ID3D11UnorderedAccessView> {
        if self.bgra_uav.is_none() {
            let mut uav: Option<ID3D11UnorderedAccessView> = None;
            unsafe {
                self.d3d_resources
                    .device
                    .CreateUnorderedAccessView(texture, None, Some(&mut uav))?;
            }
            self.bgra_uav = uav;
        }
        Ok(self.bgra_uav.as_ref().unwrap().clone())
    }

    pub fn context(&self) -> ID3D11DeviceContext {
        self.d3d_resources.context.clone()
    }

    pub fn device(&self) -> ID3D11Device {
        self.d3d_resources.device.clone()
    }

    /// 解像度変更によりテクスチャの再設定が必要かチェック
    pub fn needs_reconfigure(
        &self,
        src_width: u32,
        src_height: u32,
        dst_width: u32,
        dst_height: u32,
    ) -> bool {
        unsafe {
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            
            // 出力テクスチャをチェック
            if let Some(tex) = &self.output_texture {
                tex.GetDesc(&mut desc);
                if desc.Width != dst_width || desc.Height != dst_height {
                    return true;
                }
            } else {
                return true;
            }

            // 入力テクスチャ(RGBA)をチェック
            if let Some(tex) = &self.rgba_texture {
                tex.GetDesc(&mut desc);
                if desc.Width != src_width || desc.Height != src_height {
                    return true;
                }
            } else {
                return true;
            }

            false
        }
    }

    /// 全てのテクスチャをクリア（再設定時に使用）
    pub fn clear(&mut self) {
        self.rgba_texture = None;
        self.bgra_texture = None;
        self.output_texture = None;
        self.rgba_srv = None;
        self.bgra_uav = None;
    }
}
