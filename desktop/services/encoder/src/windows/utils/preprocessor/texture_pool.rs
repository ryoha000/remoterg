use anyhow::Result;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device, ID3D11DeviceContext, ID3D11ShaderResourceView, ID3D11Texture2D,
    ID3D11UnorderedAccessView,
};

use crate::windows::utils::d3d::D3D11Resources;
use super::texture;

/// キャッシュされたテクスチャとサイズ情報
struct CachedTexture {
    texture: ID3D11Texture2D,
    width: u32,
    height: u32,
}

/// プリプロセッサ用のD3D11テクスチャを管理
pub struct TexturePool {
    d3d_resources: D3D11Resources,
    rgba_texture: Option<CachedTexture>,
    bgra_texture: Option<CachedTexture>,
    output_texture: Option<CachedTexture>,
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
        let needs_recreate = if let Some(cached) = &self.rgba_texture {
            cached.width != width || cached.height != height
        } else {
            true
        };

        if needs_recreate {
            let texture = texture::create_rgba_texture(&self.d3d_resources.device, width, height)?;
            self.rgba_texture = Some(CachedTexture {
                texture,
                width,
                height,
            });
            // 依存するViewを無効化
            self.rgba_srv = None;
        }

        let cached = self.rgba_texture.as_ref().unwrap();
        texture::upload_rgba_data(
            &self.d3d_resources.context,
            &cached.texture,
            rgba_data,
            width,
            height,
        );

        Ok(cached.texture.clone())
    }

    /// BGRAテクスチャを作成（変換先）
    pub fn ensure_bgra_texture(&mut self, width: u32, height: u32) -> Result<ID3D11Texture2D> {
        let needs_recreate = if let Some(cached) = &self.bgra_texture {
            cached.width != width || cached.height != height
        } else {
            true
        };

        if needs_recreate {
            let texture = texture::create_bgra_texture(&self.d3d_resources.device, width, height)?;
            self.bgra_texture = Some(CachedTexture {
                texture,
                width,
                height,
            });
            // 依存するViewを無効化
            self.bgra_uav = None;
        }

        Ok(self.bgra_texture.as_ref().unwrap().texture.clone())
    }

    /// NV12出力テクスチャを作成
    pub fn ensure_output_texture(&mut self, width: u32, height: u32) -> Result<ID3D11Texture2D> {
        let needs_recreate = if let Some(cached) = &self.output_texture {
            cached.width != width || cached.height != height
        } else {
            true
        };

        if needs_recreate {
            let texture = texture::create_nv12_texture(&self.d3d_resources.device, width, height)?;
            self.output_texture = Some(CachedTexture {
                texture,
                width,
                height,
            });
        }

        Ok(self.output_texture.as_ref().unwrap().texture.clone())
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
        // 出力テクスチャをチェック
        if let Some(cached) = &self.output_texture {
            if cached.width != dst_width || cached.height != dst_height {
                return true;
            }
        } else {
            return true;
        }

        // 入力テクスチャ(RGBA)をチェック
        if let Some(cached) = &self.rgba_texture {
            if cached.width != src_width || cached.height != src_height {
                return true;
            }
        } else {
            return true;
        }

        false
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
