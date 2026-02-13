use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;

/// キャッシュされたテクスチャとサイズ情報
pub struct CachedTexture {
    texture: ID3D11Texture2D,
    width: u32,
    height: u32,
}

impl CachedTexture {
    /// 新しいCachedTextureを作成
    pub fn new(texture: ID3D11Texture2D, width: u32, height: u32) -> Self {
        Self {
            texture,
            width,
            height,
        }
    }

    /// サイズ変更が必要かチェック
    pub fn needs_resize(&self, width: u32, height: u32) -> bool {
        self.width != width || self.height != height
    }

    /// テクスチャへの参照を取得
    pub fn texture(&self) -> &ID3D11Texture2D {
        &self.texture
    }

    /// 幅を取得
    pub fn width(&self) -> u32 {
        self.width
    }

    /// 高さを取得
    pub fn height(&self) -> u32 {
        self.height
    }

    /// テクスチャを消費して取得
    pub fn into_texture(self) -> ID3D11Texture2D {
        self.texture
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{D3D11Device, TextureBuilder};

    #[test]
    fn test_cached_texture_creation() {
        let device = D3D11Device::new().expect("Failed to create device");
        let texture = TextureBuilder::rgba_input()(64, 64)
            .build(&device)
            .expect("Failed to create texture");

        let cached = CachedTexture::new(texture, 64, 64);
        assert_eq!(cached.width(), 64);
        assert_eq!(cached.height(), 64);
    }

    #[test]
    fn test_needs_resize() {
        let device = D3D11Device::new().expect("Failed to create device");
        let texture = TextureBuilder::rgba_input()(64, 64)
            .build(&device)
            .expect("Failed to create texture");

        let cached = CachedTexture::new(texture, 64, 64);
        assert!(!cached.needs_resize(64, 64));
        assert!(cached.needs_resize(128, 64));
        assert!(cached.needs_resize(64, 128));
        assert!(cached.needs_resize(128, 128));
    }
}
