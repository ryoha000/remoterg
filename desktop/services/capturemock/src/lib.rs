use anyhow::Result;
use core_types::{
    CaptureBackend, CaptureCommandReceiver, CaptureConfig, CaptureFrameSender, CaptureFuture,
    CaptureMessage, Frame,
};
use std::time::Instant;
#[cfg(test)]
use tokio::sync::mpsc;
use tracing::{debug, info};

// 単色パレットとローテーション設定
const COLOR_PALETTE: [(u8, u8, u8); 10] = [
    (255, 0, 0),     // red
    (0, 255, 0),     // green
    (0, 0, 255),     // blue
    (255, 255, 255), // white
    (255, 255, 0),   // yellow
    (0, 255, 255),   // cyan
    (255, 0, 255),   // magenta
    (255, 128, 0),   // orange
    (128, 0, 255),   // purple
    (0, 0, 0),       // black
];
const COLOR_DWELL_FRAMES: u64 = 60; // 約1.3秒/色（45fps前提）
const PREGENERATED_FRAMES: usize = COLOR_PALETTE.len(); // 各色1フレームだけ持つ

/// ダミーキャプチャサービス
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

impl CaptureService {
    async fn run_inner(mut self) -> Result<()> {
        info!("CaptureService (mock) started");

        let mut is_capturing = false;
        let mut config = CaptureConfig::default();

        // 起動時に事前生成
        let mut precomputed_frames = Self::generate_frame_set(&config, PREGENERATED_FRAMES);
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
                            precomputed_frames =
                                Self::generate_frame_set(&config, PREGENERATED_FRAMES);
                            info!(
                                "Precomputed frames regenerated ({} frames) in {}ms",
                                precomputed_frames.len(),
                                regen_start.elapsed().as_millis()
                            );
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
                        let color_idx =
                            ((frame_index / COLOR_DWELL_FRAMES) as usize) % COLOR_PALETTE.len();
                        let idx = color_idx % precomputed_frames.len();
                        let mut frame = precomputed_frames[idx].clone();
                        // 実送出時刻で timestamp を更新
                        frame.timestamp = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_millis() as u64;
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
            .map(|i| Self::generate_dummy_frame(config, i))
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

    fn generate_dummy_frame(config: &CaptureConfig, frame_index: u64) -> Frame {
        let (width, height) = match &config.size {
            core_types::CaptureSize::UseSourceSize => {
                // mock では UseSourceSize の場合はデフォルトサイズを使用
                (1280, 720)
            }
            core_types::CaptureSize::Custom { width, height } => (*width, *height),
        };
        // 単色フレームを生成
        let size = (width * height * 4) as usize;
        let mut data = vec![0u8; size];

        // 画面内は完全単色。事前生成ではパレット順に1色1フレームだけ持つ
        let color_index = (frame_index as usize) % COLOR_PALETTE.len();
        let (r, g, b) = COLOR_PALETTE[color_index];

        for px in data.chunks_exact_mut(4) {
            px[0] = r;
            px[1] = g;
            px[2] = b;
            px[3] = 255;
        }

        Frame {
            width,
            height,
            data,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
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

        // 少し待ってからフレームを受信
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // フレームが生成されているか確認
        let frame = frame_rx.try_recv();
        assert!(frame.is_ok(), "Frame should be generated after start");

        // キャプチャ停止
        cmd_tx.send(CaptureMessage::Stop).await.unwrap();

        // サービスを停止
        drop(cmd_tx);
        handle.await.unwrap().unwrap();
    }

    #[test]
    fn test_dummy_frame_generation() {
        let config = CaptureConfig {
            size: core_types::CaptureSize::Custom {
                width: 640,
                height: 480,
            },
            fps: 30,
        };

        let frame = CaptureService::generate_dummy_frame(&config, 0);

        assert_eq!(frame.width, 640);
        assert_eq!(frame.height, 480);
        assert_eq!(frame.data.len(), 640 * 480 * 4);
    }
}

