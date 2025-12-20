use core_types::{EncodeJob, EncodeJobSlot, Frame};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info, span, warn, Level};
use crate::track_writer::VideoTrackState;

/// フレーム処理の統計情報
pub struct FrameStats {
    pub frames_received: u64,
    pub frames_dropped_not_ready: u64,
    pub frames_dropped_no_track: u64,
    pub frames_queued: u64,
    pub last_perf_log: Instant,
}

impl FrameStats {
    pub fn new() -> Self {
        Self {
            frames_received: 0,
            frames_dropped_not_ready: 0,
            frames_dropped_no_track: 0,
            frames_queued: 0,
            last_perf_log: Instant::now(),
        }
    }
}

/// フレームを処理してエンコードジョブを送信
pub fn process_frame(
    frame: Frame,
    track_state: Option<&mut VideoTrackState>,
    encode_job_slot: Option<&Arc<EncodeJobSlot>>,
    connection_ready: &Arc<AtomicBool>,
    keyframe_requested: &Arc<AtomicBool>,
    last_frame_ts: &mut Option<u64>,
    stats: &mut FrameStats,
    pipeline_start: Instant,
) -> Option<(u32, u32)> {
    stats.frames_received += 1;
    let interarrival_ms = last_frame_ts
        .map(|prev| frame.timestamp.saturating_sub(prev))
        .unwrap_or(0);

    debug!(
        "Received frame: {}x{} (since_last={}ms)",
        frame.width, frame.height, interarrival_ms
    );

    // ICE/DTLS 接続完了まで映像送出を保留
    if !connection_ready.load(Ordering::Relaxed) {
        stats.frames_dropped_not_ready += 1;
        if stats.frames_dropped_not_ready % 30 == 0 {
            debug!("Connection not ready yet, dropped {} frames", stats.frames_dropped_not_ready);
        }
        return None;
    }

    // フレーム処理全体を span で計測
    let process_frame_span = span!(
        Level::DEBUG,
        "process_frame",
        width = frame.width,
        height = frame.height
    );
    let _process_frame_guard = process_frame_span.enter();

    // Video trackが存在する場合、エンコードワーカーへジョブを送信
    if let Some(track_state) = track_state {
        // capture側のタイムスタンプ差分からフレーム間隔を推定（デフォルト22ms≒45fps）
        let frame_duration = if let Some(prev) = *last_frame_ts {
            let delta_ms = frame.timestamp.saturating_sub(prev).max(1);
            Duration::from_millis(delta_ms)
        } else {
            Duration::from_millis(22)
        };
        *last_frame_ts = Some(frame.timestamp);

        // 解像度変更を検出した場合はencoderを再生成する必要がある
        let resolution_changed = track_state.width != frame.width || track_state.height != frame.height;
        if resolution_changed {
            if track_state.width == 0 && track_state.height == 0 {
                info!(
                    "Observed first frame {}x{} (encoder will initialize)",
                    frame.width, frame.height
                );
            } else {
                info!(
                    "Observed frame resize {}x{} -> {}x{} (recreating encoder)",
                    track_state.width, track_state.height, frame.width, frame.height
                );
            }
            // トラック状態を更新
            track_state.width = frame.width;
            track_state.height = frame.height;
            track_state.keyframe_sent = false; // 解像度変更後はキーフレームが必要

            drop(_process_frame_guard);
            return Some((frame.width, frame.height));
        }

        // エンコードジョブ送信を span で計測
        if let Some(job_slot) = encode_job_slot {
            let queue_encode_job_span = span!(
                Level::DEBUG,
                "queue_encode_job"
            );
            let _queue_encode_job_guard = queue_encode_job_span.enter();
            let job_send_start = Instant::now();
            // キーフレーム要求が来ている場合は、フラグをリセットしてジョブに含める
            let request_keyframe = keyframe_requested.swap(false, Ordering::Relaxed);
            job_slot.set(EncodeJob {
                width: frame.width,
                height: frame.height,
                rgba: frame.data,
                duration: frame_duration,
                enqueue_at: pipeline_start,
                request_keyframe,
            });
            let job_send_dur = job_send_start.elapsed();
            drop(_queue_encode_job_guard);

            stats.frames_queued += 1;
            if job_send_dur.as_millis() > 10 {
                warn!(
                    "Encode job set took {}ms",
                    job_send_dur.as_millis()
                );
            }
        } else {
            stats.frames_dropped_no_track += 1;
            if stats.frames_dropped_no_track % 30 == 0 {
                debug!("Encoder worker not available, dropped {} frames", stats.frames_dropped_no_track);
            }
        }
    } else {
        stats.frames_dropped_no_track += 1;
        if stats.frames_dropped_no_track % 30 == 0 {
            debug!("Video track not ready, dropped {} frames", stats.frames_dropped_no_track);
        }
    }

    drop(_process_frame_guard);
    None
}

/// パフォーマンス統計をログ出力
pub fn log_performance_stats(stats: &mut FrameStats) {
    if stats.last_perf_log.elapsed().as_secs_f32() >= 5.0 {
        let elapsed_sec = stats.last_perf_log.elapsed().as_secs_f32();
        let receive_fps = stats.frames_received as f32 / elapsed_sec;
        let queue_fps = stats.frames_queued as f32 / elapsed_sec;
        tracing::info!(
            "Frame processing stats (last {}s): received={} ({:.1} fps), queued={} ({:.1} fps), dropped_not_ready={}, dropped_no_track={}",
            elapsed_sec,
            stats.frames_received,
            receive_fps,
            stats.frames_queued,
            queue_fps,
            stats.frames_dropped_not_ready,
            stats.frames_dropped_no_track
        );
        stats.frames_received = 0;
        stats.frames_queued = 0;
        stats.frames_dropped_not_ready = 0;
        stats.frames_dropped_no_track = 0;
        stats.last_perf_log = Instant::now();
    }
}

