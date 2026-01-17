use anyhow::Result;
use core_types::{
    CaptureBackend, CaptureCommandReceiver, CaptureConfig, CaptureFrameSender, CaptureFuture,
    CaptureMessage, Frame,
};
use std::time::Instant;
#[cfg(test)]
use tokio::sync::mpsc;
use tracing::{debug, info};

// グラデーションアニメーション設定
const PREGENERATED_FRAMES: usize = 90; // 45fps × 2秒 (起動高速化のため削減)

/// HSVからRGBに変換
/// h: 色相 (0.0-360.0)
/// s: 彩度 (0.0-1.0)
/// v: 明度 (0.0-1.0)
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let c = v * s;
    let h_prime = h / 60.0;
    let x = c * (1.0 - ((h_prime % 2.0) - 1.0).abs());
    let m = v - c;

    let (r, g, b) = match h_prime as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };

    (
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}

/// ダミーキャプチャサービス
pub struct CaptureService {
    frame_tx: CaptureFrameSender,
    command_rx: CaptureCommandReceiver,
    precomputed_frames: Vec<Frame>,
}

impl CaptureBackend for CaptureService {
    fn new(frame_tx: CaptureFrameSender, command_rx: CaptureCommandReceiver) -> Self {
        // 起動時のブロッキングを防ぐため、ここではフレーム生成を行わない
        Self {
            frame_tx,
            command_rx,
            precomputed_frames: Vec::new(),
        }
    }

    fn run(self) -> CaptureFuture {
        Box::pin(async move { self.run_inner().await })
    }
}

impl CaptureService {
    async fn run_inner(mut self) -> Result<()> {
        info!("CaptureService (mock) started");

        let mut is_capturing = false;
        let mut config = CaptureConfig::default();

        // 初回フレーム生成（バックグラウンドで実行）
        if self.precomputed_frames.is_empty() {
            info!("Generating initial mock frames in background...");
            let config_clone = config.clone();
            let frames = tokio::task::spawn_blocking(move || {
                Self::generate_frame_set(&config_clone, PREGENERATED_FRAMES)
            })
            .await?;
            self.precomputed_frames = frames;
            info!(
                "Initial mock frames generated ({} frames)",
                self.precomputed_frames.len()
            );
        }

        // 事前生成済みフレームを使用
        let mut precomputed_frames = self.precomputed_frames;
        let mut frame_index: u64 = 0;
        let mut last_frame_log = Instant::now();
        loop {
            tokio::select! {
                // コマンド受信
                msg = self.command_rx.recv() => {
                    match msg {
                        Some(CaptureMessage::Start { hwnd }) => {
                            info!("Start capture (mock) for HWND: {}", hwnd);
                            is_capturing = true;
                        }
                        Some(CaptureMessage::Stop) => {
                            info!("Stop capture (mock)");
                            is_capturing = false;
                        }
                        Some(CaptureMessage::UpdateConfig { size, fps }) => {
                            match &size {
                                core_types::CaptureSize::UseSourceSize => {
                                    info!("Update config (mock): UseSourceSize @ {}fps", fps);
                                }
                                core_types::CaptureSize::Custom { width, height } => {
                                    info!("Update config (mock): {}x{} @ {}fps", width, height, fps);
                                }
                            }
                            config.size = size;
                            config.fps = fps;
                            frame_index = 0;
                            let regen_start = Instant::now();
                            
                            // 設定変更時もバックグラウンドで再生成
                            let config_clone = config.clone();
                            let new_frames = tokio::task::spawn_blocking(move || {
                                Self::generate_frame_set(&config_clone, PREGENERATED_FRAMES)
                            }).await?;
                            precomputed_frames = new_frames;

                            info!(
                                "Precomputed frames regenerated ({} frames) in {}ms",
                                precomputed_frames.len(),
                                regen_start.elapsed().as_millis()
                            );
                        }
                        Some(CaptureMessage::RequestFrame { tx }) => {
                            info!("RequestFrame (mock)");
                             if !precomputed_frames.is_empty() {
                                let idx = (frame_index as usize) % precomputed_frames.len();
                                let mut frame = precomputed_frames[idx].clone();
                                 let now = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap();
                                frame.windows_timespan = now.as_nanos() as u64 / 100;
                                let _ = tx.send(frame);
                            } else {
                                // No frames available yet
                                tracing::warn!("RequestFrame (mock): No frames available");
                            }
                        }
                        None => {
                            debug!("Command channel closed");
                            break;
                        }
                    }
                }
                // ダミーフレーム生成
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(1000 / config.fps.max(1) as u64)) => {
                    if is_capturing {
                        let frame_start = Instant::now();
                        if precomputed_frames.is_empty() {
                             continue;
                        }
                        let idx = (frame_index as usize) % precomputed_frames.len();
                        let mut frame = precomputed_frames[idx].clone();
                        // 実送出時刻で windows_timespan を更新（100ナノ秒単位に変換）
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap();
                        frame.windows_timespan = now.as_nanos() as u64 / 100;
                        frame_index = frame_index.wrapping_add(1);
                        let send_start = Instant::now();
                        if let Err(e) = self.frame_tx.send(frame).await {
                            tracing::error!("Failed to send frame: {}", e);
                            break;
                        }
                        let send_dur = send_start.elapsed();
                        let total_dur = frame_start.elapsed();

                        // フレーム処理時間をデバッグ出力（事前生成なので send/total のみ）
                        debug!(
                            "capture frame idx={} reuse precomputed send={}ms total={}ms",
                            frame_index,
                            send_dur.as_millis(),
                            total_dur.as_millis(),
                        );

                        // ある程度の間隔で概要ログも出す
                        if last_frame_log.elapsed().as_secs_f32() >= 5.0 {
                            info!(
                                "capture running (mock): last_frame_idx={} send={}ms total={}ms (precomputed {})",
                                frame_index,
                                send_dur.as_millis(),
                                total_dur.as_millis(),
                                precomputed_frames.len(),
                            );
                            last_frame_log = Instant::now();
                        }
                    }
                }
            }
        }

        info!("CaptureService (mock) stopped");
        Ok(())
    }

    fn generate_frame_set(config: &CaptureConfig, count: usize) -> Vec<Frame> {
        let start = Instant::now();
        let frames: Vec<Frame> = (0..count as u64)
            .map(|i| Self::generate_gradient_frame(config, i))
            .collect();
        let (width, height) = match &config.size {
            core_types::CaptureSize::UseSourceSize => (0, 0),
            core_types::CaptureSize::Custom { width, height } => (*width, *height),
        };
        info!(
            "Pre-generated {} frames for {}x{} @{}fps in {}ms",
            frames.len(),
            width,
            height,
            config.fps,
            start.elapsed().as_millis()
        );
        frames
    }

    fn generate_gradient_frame(config: &CaptureConfig, frame_index: u64) -> Frame {
        let (width, height) = match &config.size {
            core_types::CaptureSize::UseSourceSize => {
                // mock では UseSourceSize の場合はデフォルトサイズを使用
                (1280, 720)
            }
            core_types::CaptureSize::Custom { width, height } => (*width, *height),
        };

        let size = (width * height * 4) as usize;
        let mut data = vec![0u8; size];

        // フレームごとの色相オフセット (360度 / 90フレーム = 4度/フレーム)
        // 元は 450フレームで360度だったので 0.8度/フレーム
        let frame_hue_offset = (frame_index as f32 / PREGENERATED_FRAMES as f32) * 360.0;

        for y in 0..height {
            for x in 0..width {
                // 横方向のグラデーション
                let gradient_hue = (x as f32 / width as f32) * 360.0;
                let pixel_hue = (gradient_hue + frame_hue_offset) % 360.0;

                let (r, g, b) = hsv_to_rgb(pixel_hue, 1.0, 0.7);

                let pixel_offset = ((y * width + x) * 4) as usize;
                data[pixel_offset] = r;
                data[pixel_offset + 1] = g;
                data[pixel_offset + 2] = b;
                data[pixel_offset + 3] = 255; // Alpha
            }
        }

        Frame {
            width,
            height,
            data,
            windows_timespan: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64
                / 100,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_capture_service_start_stop() {
        let (frame_tx, mut frame_rx) = mpsc::channel(10);
        let (cmd_tx, cmd_rx) = mpsc::channel(10);

        let service = CaptureService::new(frame_tx, cmd_rx);
        let handle = tokio::spawn(async move { service.run().await });

        // キャプチャ開始
        cmd_tx
            .send(CaptureMessage::Start { hwnd: 12345 })
            .await
            .unwrap();

        // フレームが生成されるまで待つ
        // 初期生成 + 最初のフレーム送信
        let frame =
            tokio::time::timeout(tokio::time::Duration::from_secs(10), frame_rx.recv()).await;
        assert!(frame.is_ok(), "Frame should be generated within timeout");
        assert!(
            frame.unwrap().is_some(),
            "Frame should be generated after start"
        );

        // キャプチャ停止
        cmd_tx.send(CaptureMessage::Stop).await.unwrap();

        // サービスを停止
        drop(cmd_tx);
        handle.await.unwrap().unwrap();
    }

    #[test]
    fn test_gradient_frame_generation() {
        let config = CaptureConfig {
            size: core_types::CaptureSize::Custom {
                width: 640,
                height: 480,
            },
            fps: 30,
        };

        let frame = CaptureService::generate_gradient_frame(&config, 0);

        assert_eq!(frame.width, 640);
        assert_eq!(frame.height, 480);
        assert_eq!(frame.data.len(), 640 * 480 * 4);

        // フレーム0と中間フレームで異なることを確認
        let mid_frame = PREGENERATED_FRAMES as u64 / 2;
        let frame2 = CaptureService::generate_gradient_frame(&config, mid_frame);
        assert_ne!(frame.data, frame2.data);
    }
}
