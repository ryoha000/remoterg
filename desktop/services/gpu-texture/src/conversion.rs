use crate::{D3D11Device, SharedTexture};
use anyhow::Result;

/// RGBA データから shared texture handle を作成
pub fn rgba_to_shared_handle(rgba: &[u8], width: u32, height: u32) -> Result<u64> {
    let device = D3D11Device::new()?;
    let texture = SharedTexture::from_rgba(&device, rgba, width, height)?;
    Ok(texture.handle())
}

/// Shared handle から RGBA データを読み取る
pub fn shared_handle_to_rgba(handle: u64, _width: u32, _height: u32) -> Result<Vec<u8>> {
    let device = D3D11Device::new()?;
    let texture = SharedTexture::from_handle(&device, handle)?;
    texture.to_rgba(&device)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// テスト用のグラデーション RGBA データを生成
    fn generate_test_rgba(width: u32, height: u32) -> Vec<u8> {
        let mut data = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let r = ((x * 255) / width.max(1)) as u8;
                let g = ((y * 255) / height.max(1)) as u8;
                let b = ((x + y) % 256) as u8;
                let a = 255u8;
                data.push(r);
                data.push(g);
                data.push(b);
                data.push(a);
            }
        }
        data
    }

    #[test]
    fn test_rgba_roundtrip() {
        let width = 64;
        let height = 64;
        let original_rgba = generate_test_rgba(width, height);

        // デバイスを作成（同じデバイスを使用することが重要）
        let device = D3D11Device::new().expect("Failed to create device");

        // RGBA → Shared Texture
        let texture = SharedTexture::from_rgba(&device, &original_rgba, width, height)
            .expect("Failed to create texture");

        // Handle → RGBA (同じデバイスを使用)
        let retrieved_rgba = texture
            .to_rgba(&device)
            .expect("Failed to retrieve RGBA from handle");

        // データが一致するか確認
        assert_eq!(
            original_rgba.len(),
            retrieved_rgba.len(),
            "Data length mismatch"
        );
        assert_eq!(
            original_rgba, retrieved_rgba,
            "Data mismatch after roundtrip"
        );
    }

    #[test]
    fn test_shared_handle() {
        let width = 128;
        let height = 128;
        let rgba = generate_test_rgba(width, height);

        let handle =
            rgba_to_shared_handle(&rgba, width, height).expect("Failed to create shared handle");

        assert_ne!(handle, 0, "Handle should not be zero");
    }

    #[test]
    fn test_device_creation() {
        let device = D3D11Device::new().expect("Failed to create D3D11 device");
        // デバイスが作成されたことを確認
        let _ = device.device();
        let _ = device.context();
    }
}
