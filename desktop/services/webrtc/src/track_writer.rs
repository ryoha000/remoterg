use core_types::{EncodeResult, VideoEncoderFactory};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tracing::{error, info, span, Level};
use webrtc_rs::media::Sample;
use webrtc_rs::track::track_local::track_local_static_sample::TrackLocalStaticSample;
use bytes::Bytes;

/// Video trackとエンコーダーの状態
pub struct VideoTrackState {
    pub track: Arc<TrackLocalStaticSample>,
    pub width: u32,
    pub height: u32,
    pub keyframe_sent: bool, // 初期キーフレーム送信済みか
    pub encoder_factory: Arc<dyn VideoEncoderFactory>,
}

/// エンコード結果を処理してtrackに書き込み
pub async fn process_encode_result(
    result: EncodeResult,
    track_state: &mut VideoTrackState,
    frame_count: &mut u64,
    last_frame_log: &mut Instant,
) {
    if result.is_keyframe {
        track_state.keyframe_sent = true;
    }

    let sample_size = result.sample_data.len();
    let sample = Sample {
        data: Bytes::from(result.sample_data),
        duration: result.duration,
        ..Default::default()
    };

    // サンプル書き込みを span で計測
    let write_sample_span = span!(
        Level::DEBUG,
        "write_sample",
        width = result.width,
        height = result.height,
        sample_size = sample_size,
        is_keyframe = result.is_keyframe
    );
    let _write_sample_guard = write_sample_span.enter();
    match track_state.track.write_sample(&sample).await {
        Ok(_) => {
            drop(_write_sample_guard);
            *frame_count += 1;
            let elapsed = last_frame_log.elapsed();
            if elapsed.as_secs_f32() >= 5.0 {
                info!("Video frames sent: {} (last {}s)", *frame_count, elapsed.as_secs());
                *frame_count = 0;
                *last_frame_log = Instant::now();
            }
        }
        Err(e) => {
            drop(_write_sample_guard);
            error!("Failed to write sample to track: {}", e);
        }
    }
}

/// キーフレーム要求を処理
pub fn handle_keyframe_request(
    track_state: &mut VideoTrackState,
    keyframe_requested: &Arc<AtomicBool>,
) {
    info!("Keyframe requested via RTCP");
    keyframe_requested.store(true, Ordering::Relaxed);
    track_state.keyframe_sent = false;
}

