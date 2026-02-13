use anyhow::{Context, Result};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_SDK_VERSION,
};

/// D3D11 デバイスとコンテキストを管理
pub struct D3D11Device {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
}

impl D3D11Device {
    /// 既存のデバイスとコンテキストから D3D11Device を作成
    pub fn from_raw(device: ID3D11Device, context: ID3D11DeviceContext) -> Self {
        Self { device, context }
    }

    /// 新しい D3D11 デバイスを作成
    pub fn new() -> Result<Self> {
        unsafe {
            let mut device: Option<ID3D11Device> = None;
            let mut context: Option<ID3D11DeviceContext> = None;

            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                windows::Win32::Foundation::HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT, // windows-captureと同じ
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
            .context("Failed to create D3D11 device")?;

            let device = device.context("Device is None")?;
            let context = context.context("Context is None")?;

            Ok(Self::from_raw(device, context))
        }
    }

    /// デバイスを取得
    pub fn device(&self) -> &ID3D11Device {
        &self.device
    }

    /// コンテキストを取得
    pub fn context(&self) -> &ID3D11DeviceContext {
        &self.context
    }
}

impl Default for D3D11Device {
    fn default() -> Self {
        Self::new().expect("Failed to create default D3D11Device")
    }
}
