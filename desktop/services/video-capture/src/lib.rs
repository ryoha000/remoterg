use anyhow::Result;
use core_types::{
    CaptureBackend, CaptureCommandReceiver, CaptureConfig, CaptureFrameSender, CaptureFuture,
    CaptureMessage, Frame,
};
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};
use tokio::time::Duration;
use tracing::{debug, error, info, span, Level};
use windows_capture::capture::{
    CaptureControl, Context as CaptureContext, GraphicsCaptureApiHandler,
};
use windows_capture::frame::Frame as WindowsFrame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};
use windows_capture::window::Window;

/// 実キャプチャサービス（windows-captureクレートによる HWND キャプチャ）
pub struct CaptureService {
    frame_tx: CaptureFrameSender,
    command_rx: CaptureCommandReceiver,
}

impl CaptureBackend for CaptureService {
    fn new(frame_tx: CaptureFrameSender, command_rx: CaptureCommandReceiver) -> Self {
        Self {
            frame_tx,
            command_rx,
        }
    }

    fn run(self) -> CaptureFuture {
        Box::pin(async move { self.run_inner().await })
    }
}

use windows::core::Interface;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Texture2D, D3D11_BIND_SHADER_RESOURCE, D3D11_RESOURCE_MISC_SHARED, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT,
};
use windows::Win32::Graphics::Dxgi::IDXGIResource;

/// windows-captureのハンドラ実装
struct CaptureHandler {
    frame_tx: mpsc::Sender<Frame>,
    screenshot_tx: Arc<Mutex<Option<oneshot::Sender<Frame>>>>,
    last_captured_frame: Arc<Mutex<Option<Frame>>>,
    _config: CaptureConfig,
    frame_counter: u64,
    shared_texture: Option<ID3D11Texture2D>,
    texture_desc: Option<D3D11_TEXTURE2D_DESC>,
}

impl GraphicsCaptureApiHandler for CaptureHandler {
    type Flags = CaptureConfigWithSender;
    type Error = anyhow::Error;

    fn new(ctx: CaptureContext<Self::Flags>) -> Result<Self, Self::Error> {
        info!("CaptureHandler::new called");
        Ok(Self {
            frame_tx: ctx.flags.frame_tx.clone(),
            screenshot_tx: ctx.flags.screenshot_tx.clone(),
            last_captured_frame: ctx.flags.last_captured_frame.clone(),
            _config: ctx.flags.config.clone(),
            frame_counter: 0,
            shared_texture: None,
            texture_desc: None,
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut WindowsFrame,
        _capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        self.frame_counter += 1;
        let frame_id = self.frame_counter;

        let frame_span = span!(Level::DEBUG, "frame_processing", frame_id = frame_id);
        let _frame_guard = frame_span.enter();

        let width = frame.width();
        let height = frame.height();
        let desc = frame.desc(); // D3D11_TEXTURE2D_DESC

        // Check if we need to recreate the shared texture
        if self.shared_texture.is_none()
            || self
                .texture_desc
                .as_ref()
                .map(|d| d.Width != width || d.Height != height || d.Format != desc.Format)
                .unwrap_or(true)
        {
            info!(
                "Creating/Recreating shared texture: {}x{} format:{:?}",
                width, height, desc.Format
            );

            let device = frame.device();
            let new_desc = D3D11_TEXTURE2D_DESC {
                Width: width,
                Height: height,
                MipLevels: 1,
                ArraySize: 1,
                Format: desc.Format,
                SampleDesc: desc.SampleDesc,
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                CPUAccessFlags: 0,
                MiscFlags: D3D11_RESOURCE_MISC_SHARED.0 as u32, // Important!
            };

            let mut texture: Option<ID3D11Texture2D> = None;
            unsafe {
                if let Err(e) = device.CreateTexture2D(&new_desc, None, Some(&mut texture)) {
                    error!("Failed to create shared texture: {:?}", e);
                    // Fallback or error handling
                }
            };

            if let Some(tex) = texture {
                self.shared_texture = Some(tex);
                self.texture_desc = Some(new_desc);
            }
        }

        let mut texture_handle: Option<u64> = None;

        // Copy to shared texture
        if let Some(shared_tex) = &self.shared_texture {
            let context = frame.device_context();
            let src_texture = frame.as_raw_texture();

            unsafe {
                context.CopyResource(shared_tex, src_texture);
            }

            // Get shared handle
            if let Ok(dxgi_resource) = shared_tex.cast::<IDXGIResource>() {
                if let Ok(handle) = unsafe { dxgi_resource.GetSharedHandle() } {
                    texture_handle = Some(handle.0 as u64);
                }
            }
        }

        // Check for screenshot request first to decide if we need CPU buffer
        let need_cpu_buffer = if let Ok(guard) = self.screenshot_tx.try_lock() {
            guard.is_some()
        } else {
            false
        };

        // If screenshot is requested, read buffer. Otherwise use empty.
        // Also capture buffer if we failed to get a texture handle (fallback)
        let final_data: Arc<Vec<u8>> = if need_cpu_buffer || texture_handle.is_none() {
            // ... existing CPU buffer logic ...
            // FrameBufferを取得してRGBAデータを読み取る
            let mut frame_buffer = frame.buffer()?;
            let row_pitch = frame_buffer.row_pitch() as usize;
            let width_usize = frame_buffer.width() as usize;
            let height_usize = frame_buffer.height() as usize;

            let raw_buffer = frame_buffer.as_raw_buffer();
            let row_size = width_usize * 4;

            let mut buffer = Vec::with_capacity(row_size * height_usize);

            for y in 0..height_usize {
                let start = y * row_pitch;
                let end = start + row_size;
                if end <= raw_buffer.len() {
                    buffer.extend_from_slice(&raw_buffer[start..end]);
                }
            }

            Arc::new(buffer)
        } else {
            Arc::new(Vec::new())
        };

        let timespan = frame.timestamp()?;
        let duration: std::time::Duration = timespan.into();
        let windows_timespan = (duration.as_nanos() / 100) as u64;

        let core_frame = Frame {
            width,
            height,
            data: final_data.clone(),
            windows_timespan,
            id: frame_id,
            texture_handle,
        };

        // 最新フレームをキャッシュ（スクリーンショット用）
        if let Ok(mut guard) = self.last_captured_frame.lock() {
            *guard = Some(core_frame.clone());
        }

        // スクリーンショット要求があるかチェックして処理
        if let Ok(mut guard) = self.screenshot_tx.lock() {
            if let Some(tx) = guard.take() {
                info!("Handling screenshot request in on_frame_arrived");
                let _ = tx.send(core_frame.clone());
            }
        }

        // フレーム送信を span で計測
        let send_span = span!(Level::DEBUG, "send_frame", frame_id = frame_id);
        let _send_guard = send_span.enter();

        match self.frame_tx.try_send(core_frame) {
            Ok(_) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                debug!("Frame dropped (channel full)");
                tracing::trace!(name: "frame_drop", reason = "channel_full", frame_id = frame_id);
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                error!("Failed to send frame: channel closed");
                tracing::trace!(name: "frame_drop", reason = "channel_closed", frame_id = frame_id);
            }
        }

        drop(_send_guard);
        drop(_frame_guard);

        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        info!("Capture session closed");
        Ok(())
    }
}

/// 画像リサイズ処理の実装（ベンチマーク用に公開）
pub fn resize_image_impl(
    src_data: &[u8],
    src_width: u32,
    src_height: u32,
    dst_width: u32,
    dst_height: u32,
) -> Result<Vec<u8>> {
    let dst_stride = dst_width * 4;
    let mut dst_data = vec![0u8; (dst_stride * dst_height) as usize];

    for y in 0..dst_height {
        let src_y = (y * src_height) / dst_height;
        for x in 0..dst_width {
            let src_x = (x * src_width) / dst_width;

            let src_offset = (src_y * src_width + src_x) * 4;
            let dst_offset = (y * dst_width + x) * 4;

            if (src_offset + 4) as usize <= src_data.len()
                && (dst_offset + 4) as usize <= dst_data.len()
            {
                dst_data[dst_offset as usize..(dst_offset + 4) as usize]
                    .copy_from_slice(&src_data[src_offset as usize..(src_offset + 4) as usize]);
            }
        }
    }

    Ok(dst_data)
}

impl CaptureService {
    async fn run_inner(mut self) -> Result<()> {
        info!("CaptureService (windows-capture) started");

        let mut capture_control: Option<CaptureControl<CaptureHandler, anyhow::Error>> = None;
        let mut target_hwnd: Option<u64> = None;
        let mut config = CaptureConfig::default();

        // スクリーンショット要求を保持する共有ステート
        let screenshot_req: Arc<Mutex<Option<oneshot::Sender<Frame>>>> = Arc::new(Mutex::new(None));
        // 最新フレームのキャッシュ（共有）
        let last_captured_frame: Arc<Mutex<Option<Frame>>> = Arc::new(Mutex::new(None));

        loop {
            tokio::select! {
                msg = self.command_rx.recv() => {
                    match msg {
                        Some(CaptureMessage::Start { hwnd }) => {
                            info!("Start capture for HWND: {hwnd}");
                            target_hwnd = Some(hwnd);

                            // 既存のキャプチャを停止
                            if let Some(control) = capture_control.take() {
                                if let Err(e) = control.stop() {
                                    error!("Failed to stop previous capture: {:?}", e);
                                }
                            }

                            // 新しいキャプチャセッションを開始
                            match Self::start_capture(hwnd, &config, self.frame_tx.clone(), screenshot_req.clone(), last_captured_frame.clone()).await {
                                Ok(control) => {
                                    capture_control = Some(control);
                                    info!("Capture started successfully");
                                }
                                Err(e) => {
                                    error!("Failed to start capture: {:?}", e);
                                }
                            }
                        }
                        Some(CaptureMessage::Stop) => {
                            info!("Stop capture");
                            if let Some(control) = capture_control.take() {
                                if let Err(e) = control.stop() {
                                    error!("Failed to stop capture: {:?}", e);
                                }
                            }
                        }
                        Some(CaptureMessage::UpdateConfig { size, fps }) => {
                            match &size {
                                core_types::CaptureSize::UseSourceSize => {
                                    info!("Update config: UseSourceSize @ {}fps", fps);
                                }
                                core_types::CaptureSize::Custom { width, height } => {
                                    info!("Update config: {}x{} @ {}fps", width, height, fps);
                                }
                            }
                            config.size = size;
                            config.fps = fps.max(1);

                            // キャプチャ中ならセッションを再作成
                            if capture_control.is_some() {
                                if let Some(hwnd_raw) = target_hwnd {
                                    // 既存のキャプチャを停止
                                    if let Some(control) = capture_control.take() {
                                        if let Err(e) = control.stop() {
                                            error!("Failed to stop capture session: {:?}", e);
                                        }
                                    }

                                    // 新しい設定で再開
                                    match Self::start_capture(hwnd_raw, &config, self.frame_tx.clone(), screenshot_req.clone(), last_captured_frame.clone()).await {
                                        Ok(control) => {
                                            capture_control = Some(control);
                                            info!("Capture restarted with new config");
                                        }
                                        Err(e) => {
                                            error!("Failed to restart capture session: {:?}", e);
                                        }
                                    }
                                }
                            }
                        }
                        Some(CaptureMessage::RequestFrame { tx }) => {
                            info!("RequestFrame received");
                            // まずキャッシュをチェック
                            let cached_frame = if let Ok(guard) = last_captured_frame.lock() {
                                guard.clone()
                            } else {
                                None
                            };

                            if let Some(frame) = cached_frame {
                                info!("Returning cached frame for screenshot");
                                let _ = tx.send(frame);
                            } else {
                                info!("No cached frame, queuing for next frame");
                                if let Ok(mut guard) = screenshot_req.lock() {
                                    *guard = Some(tx);
                                } else {
                                    error!("Failed to lock screenshot_req mutex");
                                }
                            }
                        }
                        None => {
                            debug!("Command channel closed");
                            break;
                        }
                    }
                }
            }
        }

        // クリーンアップ
        if let Some(control) = capture_control.take() {
            let _ = control.stop();
        }

        info!("CaptureService (windows-capture) stopped");
        Ok(())
    }

    async fn start_capture(
        hwnd: u64,
        config: &CaptureConfig,
        frame_tx: mpsc::Sender<Frame>,
        screenshot_tx: Arc<Mutex<Option<oneshot::Sender<Frame>>>>,
        last_captured_frame: Arc<Mutex<Option<Frame>>>,
    ) -> Result<CaptureControl<CaptureHandler, anyhow::Error>> {
        info!("start_capture called for HWND: {hwnd}");

        // HWNDからWindowを作成
        let window = Window::from_raw_hwnd(hwnd as *mut _);
        info!("Window created from HWND");

        // Windowが有効かチェック（警告のみ、デスクトップウィンドウなどは無効でも試行）
        if !window.is_valid() {
            info!("Window is not valid for capture according to is_valid(), but will try anyway");
        } else {
            info!("Window is valid for capture");
        }

        // FPSからミリ秒への変換
        let fps_ms = Duration::from_millis(1000 / config.fps.max(1) as u64);
        info!("FPS: {}, interval: {:?}", config.fps, fps_ms);

        // Settingsを作成（Windowを直接渡す）
        let settings = Settings::new(
            window,
            CursorCaptureSettings::Default,
            DrawBorderSettings::Default,
            SecondaryWindowSettings::Default,
            MinimumUpdateIntervalSettings::Custom(fps_ms),
            DirtyRegionSettings::Default,
            ColorFormat::Rgba8,
            CaptureConfigWithSender {
                config: config.clone(),
                frame_tx,
                screenshot_tx,
                last_captured_frame,
            },
        );
        info!("Settings created");

        // キャプチャを開始（フリースレッドモード）
        // start_free_threadedはブロックする可能性があるため、tokio::task::spawn_blockingで実行
        info!("Starting capture with start_free_threaded...");
        let control_result =
            tokio::task::spawn_blocking(move || CaptureHandler::start_free_threaded(settings))
                .await
                .map_err(|e| anyhow::anyhow!("Failed to spawn capture thread: {:?}", e))?;

        let control =
            control_result.map_err(|e| anyhow::anyhow!("Failed to start capture: {:?}", e))?;
        info!("Capture started successfully, CaptureControl returned");

        Ok(control)
    }
}

/// CaptureHandlerに渡すための設定とフレーム送信チャンネルを含む構造体
#[derive(Clone)]
struct CaptureConfigWithSender {
    config: CaptureConfig,
    frame_tx: mpsc::Sender<Frame>,
    screenshot_tx: Arc<Mutex<Option<oneshot::Sender<Frame>>>>,
    last_captured_frame: Arc<Mutex<Option<Frame>>>,
}
