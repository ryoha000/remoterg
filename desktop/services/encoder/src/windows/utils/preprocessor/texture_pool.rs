use anyhow::Result;
use gpu_texture::{
    create_shader_resource_view, create_unordered_access_view, upload_data, CachedTexture,
    D3D11Device, TextureBuilder,
};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device, ID3D11DeviceContext, ID3D11ShaderResourceView, ID3D11Texture2D,
    ID3D11UnorderedAccessView,
};

use crate::windows::utils::d3d::D3D11Resources;

/// プリプロセッサ用のD3D11テクスチャを管理
pub struct TexturePool {
    d3d_resources: D3D11Resources,
    gpu_device: D3D11Device,
    rgba_texture: Option<CachedTexture>,
    bgra_texture: Option<CachedTexture>,
    output_texture: Option<CachedTexture>,
    rgba_srv: Option<ID3D11ShaderResourceView>,
    bgra_uav: Option<ID3D11UnorderedAccessView>,
}

impl TexturePool {
    pub fn new(d3d_resources: D3D11Resources) -> Self {
        let gpu_device =
            D3D11Device::from_raw(d3d_resources.device.clone(), d3d_resources.context.clone());
        Self {
            d3d_resources,
            gpu_device,
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
            cached.needs_resize(width, height)
        } else {
            true
        };

        if needs_recreate {
            let texture = TextureBuilder::rgba_input()(width, height).build(&self.gpu_device)?;
            self.rgba_texture = Some(CachedTexture::new(texture, width, height));
            // 依存するViewを無効化
            self.rgba_srv = None;
        }

        let cached = self.rgba_texture.as_ref().unwrap();
        upload_data(&self.gpu_device, cached.texture(), rgba_data, width, height);

        Ok(cached.texture().clone())
    }

    /// BGRAテクスチャを作成（変換先）
    pub fn ensure_bgra_texture(&mut self, width: u32, height: u32) -> Result<ID3D11Texture2D> {
        let needs_recreate = if let Some(cached) = &self.bgra_texture {
            cached.needs_resize(width, height)
        } else {
            true
        };

        if needs_recreate {
            let texture = TextureBuilder::bgra_target()(width, height).build(&self.gpu_device)?;
            self.bgra_texture = Some(CachedTexture::new(texture, width, height));
            // 依存するViewを無効化
            self.bgra_uav = None;
        }

        Ok(self.bgra_texture.as_ref().unwrap().texture().clone())
    }

    /// NV12出力テクスチャを作成
    pub fn ensure_output_texture(&mut self, width: u32, height: u32) -> Result<ID3D11Texture2D> {
        let needs_recreate = if let Some(cached) = &self.output_texture {
            cached.needs_resize(width, height)
        } else {
            true
        };

        if needs_recreate {
            let texture = TextureBuilder::nv12_output()(width, height).build(&self.gpu_device)?;
            self.output_texture = Some(CachedTexture::new(texture, width, height));
        }

        Ok(self.output_texture.as_ref().unwrap().texture().clone())
    }

    pub fn get_rgba_srv(&mut self, texture: &ID3D11Texture2D) -> Result<ID3D11ShaderResourceView> {
        if self.rgba_srv.is_none() {
            self.rgba_srv = Some(create_shader_resource_view(&self.gpu_device, texture)?);
        }
        Ok(self.rgba_srv.as_ref().unwrap().clone())
    }

    pub fn get_bgra_uav(&mut self, texture: &ID3D11Texture2D) -> Result<ID3D11UnorderedAccessView> {
        if self.bgra_uav.is_none() {
            self.bgra_uav = Some(create_unordered_access_view(&self.gpu_device, texture)?);
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
            if cached.needs_resize(dst_width, dst_height) {
                return true;
            }
        } else {
            return true;
        }

        // 入力テクスチャ(RGBA)をチェック
        if let Some(cached) = &self.rgba_texture {
            if cached.needs_resize(src_width, src_height) {
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
