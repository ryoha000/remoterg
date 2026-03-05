use anyhow::Result;
use bytes::Bytes;
use core_types::EncodeResult;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{error, info, span, warn, Level};
use webrtc_rs::media::Sample;
use webrtc_rs::track::track_local::track_local_static_sample::TrackLocalStaticSample;

/// Absolute Capture Time 拡張ヘッダー
/// - 8バイト: absolute_capture_timestamp (UQ32.32)
/// - 16バイト: + estimated_capture_clock_offset (Q32.32, optional)
struct AbsCaptureTimeExtension {
    ntp_timestamp: u64,
    estimated_capture_clock_offset_q32x32: Option<i64>,
}

static ACT_LOG_COUNTER: AtomicU64 = AtomicU64::new(0);

impl webrtc_rs::util::MarshalSize for AbsCaptureTimeExtension {
    fn marshal_size(&self) -> usize {
        if self.estimated_capture_clock_offset_q32x32.is_some() {
            16
        } else {
            8
        }
    }
}

impl webrtc_rs::util::Marshal for AbsCaptureTimeExtension {
    fn marshal_to(&self, buf: &mut [u8]) -> webrtc_rs::util::Result<usize> {
        let size = if self.estimated_capture_clock_offset_q32x32.is_some() {
            16
        } else {
            8
        };
        if buf.len() < size {
            return Err(webrtc_rs::util::Error::ErrBufferShort);
        }
        buf[..8].copy_from_slice(&self.ntp_timestamp.to_be_bytes());
        if let Some(offset_q32x32) = self.estimated_capture_clock_offset_q32x32 {
            buf[8..16].copy_from_slice(&offset_q32x32.to_be_bytes());
            Ok(16)
        } else {
            Ok(8)
        }
    }
}

/// エンコード結果をトラックに書き込む (abs-capture-time 拡張付き)
pub async fn write_encoded_sample(
    track: &Arc<TrackLocalStaticSample>,
    result: EncodeResult,
) -> Result<u64> {
    let sample_index = ACT_LOG_COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
    let sample_size = result.sample_data.len();

    // t_cap_ms (hostd monotonic ms) → SystemTime → NTP タイムスタンプ
    let (extensions, ntp_timestamp) = if let Some(t_cap_ms) = result.t_cap_ms {
        let capture_time = core_types::hostd_mono_ms_to_system_time(t_cap_ms);
        let ntp_ts = core_types::system_time_to_ntp(capture_time);
        let ext = webrtc_rs::rtp::extension::HeaderExtension::Custom {
            uri: "http://www.webrtc.org/experiments/rtp-hdrext/abs-capture-time".into(),
            extension: Box::new(AbsCaptureTimeExtension {
                ntp_timestamp: ntp_ts,
                // hostd では sender と capturer が同一時計のため 0 を明示する。
                // これにより受信側が local_capture_clock_offset を算出できる。
                estimated_capture_clock_offset_q32x32: Some(0),
            }),
        };
        if sample_index <= 5 || sample_index % 120 == 1 {
            info!(
                "ACT[send]: sample={} frame_id={} t_cap_ms={:.3} ntp_timestamp={} est_offset_q32x32={} ext_count={} ext_bytes={}",
                sample_index,
                result.frame_id,
                t_cap_ms,
                ntp_ts,
                0i64,
                1,
                16
            );
        }
        (vec![ext], ntp_ts)
    } else {
        if sample_index <= 5 || sample_index % 120 == 1 {
            warn!(
                "ACT[send-missing-tcap]: sample={} frame_id={} ext_count=0",
                sample_index,
                result.frame_id
            );
        }
        (vec![], 0u64)
    };

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
        is_keyframe = result.is_keyframe,
        frame_id = result.frame_id
    );
    let _write_sample_guard = write_sample_span.enter();

    match track
        .write_sample_with_extensions(&sample, &extensions)
        .await
    {
        Ok(_) => {
            drop(_write_sample_guard);
            Ok(ntp_timestamp)
        }
        Err(e) => {
            drop(_write_sample_guard);
            error!("Failed to write sample to track: {}", e);
            Err(e.into())
        }
    }
}
