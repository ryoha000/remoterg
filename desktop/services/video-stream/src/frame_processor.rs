use core_types::{EncodeJob, EncodeJobSlot, Frame, VideoEncoderFactory};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, span, warn, Level};

/// フレーム処理の統計情報
struct FrameStats {
    frames_received: u64,
    frames_dropped_not_ready: u64,
    frames_dropped_no_encoder: u64,
    frames_queued: u64,
    last_perf_log: Instant,
}

impl FrameStats {
    fn new() -> Self {
        Self {
            frames_received: 0,
            frames_dropped_not_ready: 0,
            frames_dropped_no_encoder: 0,
            frames_queued: 0,
            last_perf_log: Instant::now(),
        }
    }

    fn log_if_needed(&mut self) {
        if self.last_perf_log.elapsed().as_secs_f32() >= 5.0 {
            let elapsed_sec = self.last_perf_log.elapsed().as_secs_f32();
            let receive_fps = self.frames_received as f32 / elapsed_sec;
            let queue_fps = self.frames_queued as f32 / elapsed_sec;
            tracing::info!(
                "Frame processing stats (last {}s): received={} ({:.1} fps), queued={} ({:.1} fps), dropped_not_ready={}, dropped_no_encoder={}",
                elapsed_sec,
                self.frames_received,
                receive_fps,
                self.frames_queued,
                queue_fps,
                self.frames_dropped_not_ready,
                self.frames_dropped_no_encoder
            );
            self.frames_received = 0;
            self.frames_queued = 0;
            self.frames_dropped_not_ready = 0;
            self.frames_dropped_no_encoder = 0;
            self.last_perf_log = Instant::now();
        }
    }
}

/// フレームルーター: フレームをエンコーダーに転送する非同期タスク
pub async fn run_frame_router(
    mut frame_rx: tokio::sync::mpsc::Receiver<Frame>,
    initial_encode_job_slot: Arc<EncodeJobSlot>,
    encoder_factory: Arc<dyn VideoEncoderFactory>,
    connection_ready: Arc<AtomicBool>,
    keyframe_requested: Arc<AtomicBool>,
) {
    info!("Frame router started");

    let mut encode_job_slot = Some(initial_encode_job_slot);
    let mut current_width: u32 = 0;
    let mut current_height: u32 = 0;
    let mut last_frame_ts: Option<u64> = None;
    let mut stats = FrameStats::new();
    let mut first_frame_received = false;
    let mut first_job_queued = false;

    while let Some(frame) = frame_rx.recv().await {
        let pipeline_start = Instant::now();
        stats.frames_received += 1;

        let interarrival_ms = last_frame_ts
            .map(|prev| {
                // windows_timespan は100ナノ秒単位なので、ミリ秒に変換
                let delta_hns = frame.windows_timespan.saturating_sub(prev);
                delta_hns / 10_000
            })
            .unwrap_or(0);

        if !first_frame_received {
            info!(
                "First frame received: {}x{} (connection_ready: {})",
                frame.width,
                frame.height,
                connection_ready.load(Ordering::Relaxed)
            );
            first_frame_received = true;
        }

        debug!(
            "Received frame: {}x{} (since_last={}ms)",
            frame.width, frame.height, interarrival_ms
        );

        // ICE/DTLS 接続完了まで映像送出を保留
        if !connection_ready.load(Ordering::Relaxed) {
            stats.frames_dropped_not_ready += 1;
            if stats.frames_dropped_not_ready == 1 || stats.frames_dropped_not_ready % 100 == 0 {
                warn!(
                    "Connection not ready yet, dropped {} frames (connection_ready: false)",
                    stats.frames_dropped_not_ready
                );
            }
            continue;
        }

        // フレーム処理全体を span で計測
        let process_frame_span = span!(
            Level::DEBUG,
            "process_frame",
            width = frame.width,
            height = frame.height
        );
        let _process_frame_guard = process_frame_span.enter();

        // タイムスタンプを更新
        last_frame_ts = Some(frame.windows_timespan);

        // 解像度変更を検出した場合はencoderを再生成
        let resolution_changed = current_width != frame.width || current_height != frame.height;
        if resolution_changed {
            if current_width == 0 && current_height == 0 {
                // 最初のフレーム: エンコーダーは既に起動済みで最初のフレームを待機中
                // shutdownせずに解像度を更新するだけ
                info!(
                    "Observed first frame {}x{} (encoder already initialized and waiting)",
                    frame.width, frame.height
                );
                current_width = frame.width;
                current_height = frame.height;
                // 最初のキーフレームを要求
                keyframe_requested.store(true, Ordering::Relaxed);
            } else {
                // 実際の解像度変更: エンコーダーを再起動
                info!(
                    "Observed frame resize {}x{} -> {}x{} (recreating encoder)",
                    current_width, current_height, frame.width, frame.height
                );

                // 既存のencoderワーカーを停止
                if let Some(old_slot) = encode_job_slot.as_ref() {
                    old_slot.shutdown();
                }
                drop(encode_job_slot.take());

                // 新しいencoderワーカーを起動
                // TODO: 解像度変更時のencode_result_rx破棄問題を修正する必要がある
                let (new_slot, _new_rx) = encoder_factory.setup();
                encode_job_slot = Some(new_slot);

                current_width = frame.width;
                current_height = frame.height;
                keyframe_requested.store(true, Ordering::Relaxed);
            }
        }

        // エンコードジョブ送信を span で計測
        if let Some(job_slot) = encode_job_slot.as_ref() {
            let queue_encode_job_span = span!(Level::DEBUG, "queue_encode_job");
            let _queue_encode_job_guard = queue_encode_job_span.enter();
            let job_send_start = Instant::now();

            // キーフレーム要求が来ている場合は、フラグをリセットしてジョブに含める
            let request_keyframe = keyframe_requested.swap(false, Ordering::Relaxed);

            if !first_job_queued {
                info!(
                    "Queueing first encode job: {}x{} (keyframe: {})",
                    frame.width, frame.height, request_keyframe
                );
                first_job_queued = true;
            }

            job_slot.set(EncodeJob {
                width: frame.width,
                height: frame.height,
                rgba: frame.data,
                timestamp: frame.windows_timespan,
                enqueue_at: pipeline_start,
                request_keyframe,
            });

            let job_send_dur = job_send_start.elapsed();
            drop(_queue_encode_job_guard);

            stats.frames_queued += 1;
            if job_send_dur.as_millis() > 10 {
                warn!("Encode job set took {}ms", job_send_dur.as_millis());
            }
        } else {
            stats.frames_dropped_no_encoder += 1;
            if stats.frames_dropped_no_encoder == 1 || stats.frames_dropped_no_encoder % 10 == 0 {
                warn!(
                    "Encoder worker not available, dropped {} frames",
                    stats.frames_dropped_no_encoder
                );
            }
        }

        drop(_process_frame_guard);

        // パフォーマンス統計を定期的に出力
        stats.log_if_needed();
    }

    // クリーンアップ: エンコーダーをシャットダウン
    if let Some(job_slot) = encode_job_slot.as_ref() {
        job_slot.shutdown();
    }

    info!("Frame router stopped");
}
