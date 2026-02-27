use anyhow::Result;
use core_types::{
    CaptureBackend, CaptureCommandReceiver, CaptureConfig, CaptureFrameSender, CaptureFuture,
    CaptureMessage, Frame, ScreenshotFrame,
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

use gpu_texture::{copy_resource, D3D11Device, SharedTexture, TextureBuilder};

/// windows-captureのハンドラ実装
struct CaptureHandler {
    frame_tx: mpsc::Sender<Frame>,
    screenshot_tx: Arc<Mutex<Option<oneshot::Sender<ScreenshotFrame>>>>,
    last_captured_frame: Arc<Mutex<Option<ScreenshotFrame>>>,
    _config: CaptureConfig,
    frame_counter: u64,
    shared_texture: Option<SharedTexture>,
    closed_tx: mpsc::Sender<()>,
    is_stopping: Arc<std::sync::atomic::AtomicBool>,
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
            closed_tx: ctx.flags.closed_tx.clone(),
            is_stopping: ctx.flags.is_stopping.clone(),
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
        let needs_recreate = if let Some(st) = &self.shared_texture {
            st.width() != width || st.height() != height
        } else {
            true
        };

        if needs_recreate {
            info!(
                "Creating/Recreating shared texture: {}x{} format:{:?}",
                width, height, desc.Format
            );

            let gpu_device =
                D3D11Device::from_raw(frame.device().clone(), frame.device_context().clone());
            let texture = TextureBuilder::new(width, height)
                .format(desc.Format)
                .bind_shader_resource()
                .shared()
                .build(&gpu_device)?;

            self.shared_texture = Some(SharedTexture::new(texture)?);
        }

        let mut texture_handle: Option<u64> = None;

        // Copy to shared texture
        if let Some(shared_tex) = &self.shared_texture {
            let gpu_device =
                D3D11Device::from_raw(frame.device().clone(), frame.device_context().clone());
            copy_resource(&gpu_device, shared_tex.texture(), frame.as_raw_texture());
            texture_handle = Some(shared_tex.handle());
        }

        // タイムスタンプを計算
        let timespan = frame.timestamp()?;
        let duration: std::time::Duration = timespan.into();
        let windows_timespan = (duration.as_nanos() / 100) as u64;

        // 通常フレーム (CPU buffer なし)
        let core_frame = Frame {
            width,
            height,
            windows_timespan,
            id: frame_id,
            texture_handle,
        };

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

        // スクリーンショット要求がある場合のみ CPU buffer を読み取り
        if let Ok(guard) = self.screenshot_tx.try_lock() {
            if guard.is_some() {
                // CPU buffer を読み取り
                let cpu_buffer = self.read_cpu_buffer(frame)?;
                let screenshot = ScreenshotFrame {
                    width,
                    height,
                    data: Arc::new(cpu_buffer),
                    timestamp: windows_timespan,
                };

                // キャッシュ更新
                if let Ok(mut cache) = self.last_captured_frame.lock() {
                    *cache = Some(screenshot.clone());
                }

                // 送信
                drop(guard);
                if let Ok(mut guard) = self.screenshot_tx.lock() {
                    if let Some(tx) = guard.take() {
                        info!("Handling screenshot request in on_frame_arrived");
                        let _ = tx.send(screenshot);
                    }
                }
            }
        }

        drop(_frame_guard);

        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        info!("Capture session closed (on_closed called)");
        if !self.is_stopping.load(std::sync::atomic::Ordering::SeqCst) {
            error!("Capture target lost unexpectedly!");
            let _ = self.closed_tx.try_send(());
        } else {
            info!("Capture session closed as part of intentional stop");
        }
        Ok(())
    }
}

impl CaptureHandler {
    /// CPU buffer を読み取るヘルパーメソッド
    fn read_cpu_buffer(&self, frame: &mut WindowsFrame) -> Result<Vec<u8>> {
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

        Ok(buffer)
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
        let (closed_tx, mut closed_rx) = mpsc::channel(1);
        let mut is_stopping = Arc::new(std::sync::atomic::AtomicBool::new(false));

        // スクリーンショット要求を保持する共有ステート
        let screenshot_req: Arc<Mutex<Option<oneshot::Sender<ScreenshotFrame>>>> =
            Arc::new(Mutex::new(None));
        // 最新フレームのキャッシュ（共有）
        let last_captured_frame: Arc<Mutex<Option<ScreenshotFrame>>> = Arc::new(Mutex::new(None));

        loop {
            tokio::select! {
                // キャプチャ対象の消失等を検知
                _ = closed_rx.recv() => {
                    error!("Capture target window was lost. Shutting down capture service.");
                    return Err(anyhow::anyhow!("Capture target lost"));
                }
                msg = self.command_rx.recv() => {
                    match msg {
                        Some(CaptureMessage::Start { hwnd }) => {
                            info!("Start capture for HWND: {hwnd}");
                            target_hwnd = Some(hwnd);

                            // 既存のキャプチャを停止
                            if let Some(control) = capture_control.take() {
                                is_stopping.store(true, std::sync::atomic::Ordering::SeqCst);
                                if let Err(e) = control.stop() {
                                    error!("Failed to stop previous capture: {:?}", e);
                                }
                            }
                            is_stopping = Arc::new(std::sync::atomic::AtomicBool::new(false));

                            // 新しいキャプチャセッションを開始
                            match Self::start_capture(hwnd, &config, self.frame_tx.clone(), screenshot_req.clone(), last_captured_frame.clone(), closed_tx.clone(), is_stopping.clone()).await {
                                Ok(control) => {
                                    capture_control = Some(control);
                                    info!("Capture started successfully");
                                }
                                Err(e) => {
                                    error!("Failed to start capture: {:?}", e);
                                    return Err(e); // Exit service to shutdown hostd
                                }
                            }
                        }
                        Some(CaptureMessage::Stop) => {
                            info!("Stop capture");
                            if let Some(control) = capture_control.take() {
                                is_stopping.store(true, std::sync::atomic::Ordering::SeqCst);
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
                                        is_stopping.store(true, std::sync::atomic::Ordering::SeqCst);
                                        if let Err(e) = control.stop() {
                                            error!("Failed to stop capture session: {:?}", e);
                                        }
                                    }
                                    is_stopping = Arc::new(std::sync::atomic::AtomicBool::new(false));

                                    // 新しい設定で再開
                                    match Self::start_capture(hwnd_raw, &config, self.frame_tx.clone(), screenshot_req.clone(), last_captured_frame.clone(), closed_tx.clone(), is_stopping.clone()).await {
                                        Ok(control) => {
                                            capture_control = Some(control);
                                            info!("Capture restarted with new config");
                                        }
                                        Err(e) => {
                                            error!("Failed to restart capture session (target lost?): {:?}", e);
                                            return Err(e);
                                        }
                                    }
                                }
                            }
                        }
                        Some(CaptureMessage::GetScreenshot { tx }) => {
                            info!("GetScreenshot received, queuing for next frame");
                            if let Ok(mut guard) = screenshot_req.lock() {
                                *guard = Some(tx);
                            } else {
                                error!("Failed to lock screenshot_req mutex");
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
        screenshot_tx: Arc<Mutex<Option<oneshot::Sender<ScreenshotFrame>>>>,
        last_captured_frame: Arc<Mutex<Option<ScreenshotFrame>>>,
        closed_tx: mpsc::Sender<()>,
        is_stopping: Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<CaptureControl<CaptureHandler, anyhow::Error>> {
        info!("start_capture called for HWND: {hwnd}");

        // HWNDからWindowを作成
        let window = Window::from_raw_hwnd(hwnd as *mut _);
        info!("Window created from HWND");

        // Windowが有効かチェック（警告のみ、デスクトップウィンドウなどは無効でも試行）
        if !window.is_valid() {
            info!("Window is not valid for capture according to is_valid().");
            if hwnd != 0 {
                return Err(anyhow::anyhow!("Capture target window (HWND: {}) is not valid or not found", hwnd));
            }
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
            DrawBorderSettings::WithoutBorder,
            SecondaryWindowSettings::Default,
            MinimumUpdateIntervalSettings::Custom(fps_ms),
            DirtyRegionSettings::Default,
            ColorFormat::Rgba8,
            CaptureConfigWithSender {
                config: config.clone(),
                frame_tx,
                screenshot_tx,
                last_captured_frame,
                closed_tx,
                is_stopping,
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
    screenshot_tx: Arc<Mutex<Option<oneshot::Sender<ScreenshotFrame>>>>,
    last_captured_frame: Arc<Mutex<Option<ScreenshotFrame>>>,
    closed_tx: mpsc::Sender<()>,
    is_stopping: Arc<std::sync::atomic::AtomicBool>,
}
