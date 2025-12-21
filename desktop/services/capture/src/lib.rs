use anyhow::Result;
use core_types::{
    CaptureBackend, CaptureCommandReceiver, CaptureConfig, CaptureFrameSender, CaptureFuture,
    CaptureMessage, Frame,
};
use std::sync::mpsc;
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

/// windows-captureのハンドラ実装
struct CaptureHandler {
    frame_tx: mpsc::Sender<Frame>,
    config: CaptureConfig,
}

impl GraphicsCaptureApiHandler for CaptureHandler {
    type Flags = CaptureConfigWithSender;
    type Error = anyhow::Error;

    fn new(ctx: CaptureContext<Self::Flags>) -> Result<Self, Self::Error> {
        info!("CaptureHandler::new called");
        Ok(Self {
            frame_tx: ctx.flags.frame_tx.clone(),
            config: ctx.flags.config.clone(),
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut WindowsFrame,
        _capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        debug!("on_frame_arrived called");

        // FrameBufferを取得してRGBAデータを読み取る
        let frame_buffer = frame.buffer()?;

        // パディングなしのバッファを取得
        let mut buffer = Vec::new();
        let rgba_data = frame_buffer.as_nopadding_buffer(&mut buffer);

        let src_width = frame_buffer.width();
        let src_height = frame_buffer.height();

        // リサイズが必要かチェック
        let (dst_width, dst_height) = match &self.config.size {
            core_types::CaptureSize::UseSourceSize => (src_width, src_height),
            core_types::CaptureSize::Custom { width, height } => (*width, *height),
        };

        // フレーム処理全体を span で計測
        let frame_span = span!(
            Level::DEBUG,
            "frame_processing",
            width = dst_width,
            height = dst_height,
            src_width = src_width,
            src_height = src_height
        );
        let _frame_guard = frame_span.enter();

        // リサイズが必要な場合
        let final_data = if dst_width != src_width || dst_height != src_height {
            resize_image_impl(rgba_data, src_width, src_height, dst_width, dst_height)?
        } else {
            rgba_data.to_vec()
        };

        // core_types::Frameに変換
        // frame.timestamp() は100ナノ秒単位の TimeSpan を返す
        // TimeSpan を Duration に変換してから、100ナノ秒単位の値を取得
        let timespan = frame.timestamp()?;
        let duration: std::time::Duration = timespan.into();
        // Duration から100ナノ秒単位の値を取得（as_nanos() はナノ秒単位なので、100で割る）
        let windows_timespan = (duration.as_nanos() / 100) as u64;
        let core_frame = Frame {
            width: dst_width,
            height: dst_height,
            data: final_data,
            windows_timespan,
        };

        // フレーム送信を span で計測
        let send_span = span!(Level::DEBUG, "send_frame");
        let _send_guard = send_span.enter();

        // std::sync::mpscを使って同期送信
        if let Err(e) = self.frame_tx.send(core_frame) {
            error!("Failed to send frame: {}", e);
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

        // std::sync::mpscチャンネルを作成（キャプチャスレッドからtokioタスクへのブリッジ）
        let (std_frame_tx, std_frame_rx) = mpsc::channel();
        let tokio_frame_tx = self.frame_tx.clone();

        // std::sync::mpscからtokio::sync::mpscへのブリッジタスク
        let bridge_handle = tokio::spawn(async move {
            loop {
                match std_frame_rx.recv() {
                    Ok(frame) => {
                        if let Err(e) = tokio_frame_tx.send(frame).await {
                            error!("Failed to forward frame to tokio channel: {}", e);
                            break;
                        }
                    }
                    Err(_) => {
                        debug!("std_frame_rx channel closed");
                        break;
                    }
                }
            }
        });

        let mut capture_control: Option<CaptureControl<CaptureHandler, anyhow::Error>> = None;
        let mut target_hwnd: Option<u64> = None;
        let mut config = CaptureConfig::default();

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
                            match Self::start_capture(hwnd, &config, std_frame_tx.clone()).await {
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
                                    match Self::start_capture(hwnd_raw, &config, std_frame_tx.clone()).await {
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

        // std_frame_txを閉じてブリッジタスクを終了させる
        drop(std_frame_tx);
        let _ = bridge_handle.await;

        info!("CaptureService (windows-capture) stopped");
        Ok(())
    }

    async fn start_capture(
        hwnd: u64,
        config: &CaptureConfig,
        frame_tx: mpsc::Sender<Frame>,
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
}
