use anyhow::Result;
use bytes::Bytes;
use core_types::EncodeResult;
use std::sync::Arc;
use tracing::{error, span, Level};
use webrtc_rs::media::Sample;
use webrtc_rs::track::track_local::track_local_static_sample::TrackLocalStaticSample;

/// エンコード結果をトラックに書き込む
pub async fn write_encoded_sample(
    track: &Arc<TrackLocalStaticSample>,
    result: EncodeResult,
) -> Result<()> {
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

    match track.write_sample(&sample).await {
        Ok(_) => {
            drop(_write_sample_guard);
            Ok(())
        }
        Err(e) => {
            drop(_write_sample_guard);
            error!("Failed to write sample to track: {}", e);
            Err(e.into())
        }
    }
}
