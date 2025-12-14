use anyhow::{Context, Result};
use windows::core::Interface;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_NV12;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Media::MediaFoundation::{
    IMFDXGIDeviceManager, IMFTransform, MFCreateDXGIDeviceManager, MFT_MESSAGE_SET_D3D_MANAGER,
    MF_TRANSFORM_ASYNC_UNLOCK,
};

/// D3D11 デバイスとコンテキスト、DXGI デバイスマネージャーを保持する構造体
#[derive(Clone)]
pub struct D3D11Resources {
    pub device: ID3D11Device,
    pub context: ID3D11DeviceContext,
    pub device_manager: IMFDXGIDeviceManager,
    reset_token: u32,
}

impl D3D11Resources {
    /// D3D11 デバイスと DXGI デバイスマネージャーを作成
    pub fn create() -> Result<Self> {
        let (device, context) = create_d3d11_device()?;
        let (device_manager, reset_token) = create_dxgi_device_manager(&device)?;

        Ok(Self {
            device,
            context,
            device_manager,
            reset_token,
        })
    }

    /// MFT に D3D マネージャーを設定し、非同期ロックを解除
    pub fn setup_mft(&self, transform: &IMFTransform) -> Result<()> {
        unsafe {
            // 非同期ロックを解除（D3Dマネージャー設定の前に実行）
            if let Ok(attributes) = transform.GetAttributes() {
                attributes
                    .SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1)
                    .ok()
                    .context("Failed to unlock async MFT")?;
            }

            // D3D マネージャーを設定
            transform
                .ProcessMessage(
                    MFT_MESSAGE_SET_D3D_MANAGER,
                    std::mem::transmute(self.device_manager.as_raw()),
                )
                .ok()
                .context("Failed to setup D3D manager on H.264 encoder")?;
        }

        Ok(())
    }
}

/// D3D11 デバイスとコンテキストを作成
pub fn create_d3d11_device() -> Result<(ID3D11Device, ID3D11DeviceContext)> {
    unsafe {
        let mut device: Option<ID3D11Device> = None;
        let mut context: Option<ID3D11DeviceContext> = None;

        D3D11CreateDevice(
            None, // デフォルトアダプター
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(), // ソフトウェアデバイスなし
            windows::Win32::Graphics::Direct3D11::D3D11_CREATE_DEVICE_FLAG::default(),
            None, // 機能レベル配列
            D3D11_SDK_VERSION,
            Some(&mut device),
            None, // 機能レベル
            Some(&mut context),
        )
        .ok()
        .context("Failed to create D3D11 device")?;

        let device = device.ok_or_else(|| anyhow::anyhow!("D3D11 device is None"))?;
        let context = context.ok_or_else(|| anyhow::anyhow!("D3D11 context is None"))?;

        Ok((device, context))
    }
}

/// DXGI デバイスマネージャーを作成
fn create_dxgi_device_manager(device: &ID3D11Device) -> Result<(IMFDXGIDeviceManager, u32)> {
    unsafe {
        let mut reset_token = 0u32;
        let mut device_manager: Option<IMFDXGIDeviceManager> = None;
        MFCreateDXGIDeviceManager(&mut reset_token, &mut device_manager)
            .ok()
            .context("Failed to create DXGI device manager")?;

        let device_manager =
            device_manager.ok_or_else(|| anyhow::anyhow!("Device manager is None"))?;

        // DXGI デバイスを取得
        let dxgi_device: IDXGIDevice = device
            .cast()
            .context("Failed to cast D3D11 device to DXGI device")?;

        // デバイスをリセット
        device_manager
            .ResetDevice(&dxgi_device, reset_token)
            .ok()
            .context("Failed to reset device in DXGI device manager")?;

        Ok((device_manager, reset_token))
    }
}

/// DXGI_FORMAT_NV12 の定数（必要に応じて）
pub const DXGI_FORMAT_NV12_VALUE: u32 = DXGI_FORMAT_NV12.0 as u32;
