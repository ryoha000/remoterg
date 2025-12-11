use std::num::NonZeroU32;
use std::time::{Duration, Instant};

use tokio::sync::mpsc as tokio_mpsc;
use tracing::{debug, info, warn};
use vpx_rs::{
    enc::{CodecId, Encoder, EncoderConfig, EncoderFrameFlags, RateControl, Timebase},
    EncodingDeadline, ImageFormat, Packet, YUVImageData,
};

use core_types::{EncodeJob, EncodeResult};

/// RGBA形式の画像データをI420形式に変換する
pub fn rgba_to_i420(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let y_plane = w * h;
    let uv_plane = y_plane / 4;

    let mut y = vec![0u8; y_plane];
    let mut u = vec![0u8; uv_plane];
    let mut v = vec![0u8; uv_plane];

    for j in 0..h {
        for i in 0..w {
            let idx = (j * w + i) * 4;
            let r = rgba[idx] as f32;
            let g = rgba[idx + 1] as f32;
            let b = rgba[idx + 2] as f32;

            let y_val = (0.257 * r + 0.504 * g + 0.098 * b + 16.0).round();
            y[j * w + i] = y_val.clamp(0.0, 255.0) as u8;
        }
    }

    for j in (0..h).step_by(2) {
        for i in (0..w).step_by(2) {
            let mut u_acc = 0f32;
            let mut v_acc = 0f32;
            for sj in 0..2 {
                for si in 0..2 {
                    let idx = ((j + sj) * w + (i + si)) * 4;
                    let r = rgba[idx] as f32;
                    let g = rgba[idx + 1] as f32;
                    let b = rgba[idx + 2] as f32;
                    u_acc += -0.148 * r - 0.291 * g + 0.439 * b + 128.0;
                    v_acc += 0.439 * r - 0.368 * g - 0.071 * b + 128.0;
                }
            }
            let uv_idx = (j / 2) * (w / 2) + (i / 2);
            u[uv_idx] = (u_acc / 4.0).clamp(0.0, 255.0) as u8;
            v[uv_idx] = (v_acc / 4.0).clamp(0.0, 255.0) as u8;
        }
    }

    let mut buffer = Vec::with_capacity(y_plane + 2 * uv_plane);
    buffer.extend_from_slice(&y);
    buffer.extend_from_slice(&u);
    buffer.extend_from_slice(&v);
    buffer
}

/// VPXエンコーダを作成する
pub fn create_vpx_encoder(
    codec_id: CodecId,
    width: u32,
    height: u32,
) -> anyhow::Result<Encoder<u8>> {
    let bitrate_kbps = ((width as u64 * height as u64 * 2) / 1000).max(300) as u32;
    let timebase = Timebase {
        num: NonZeroU32::new(1).expect("non-zero timebase numerator"),
        den: NonZeroU32::new(1000).expect("non-zero timebase denominator"),
    };

    let mut config = EncoderConfig::new(
        codec_id,
        width,
        height,
        timebase,
        RateControl::ConstantBitRate(bitrate_kbps),
    )?;
    // 低遅延を狙うためラグを許容しない
    config.lag_in_frames = 0;

    Encoder::new(config).map_err(|e| anyhow::anyhow!(e))
}

/// VPXエンコードワーカーを生成する
pub fn start_vpx_encode_worker(
    codec_id: CodecId,
    codec_name: &'static str,
) -> (
    std::sync::mpsc::Sender<EncodeJob>,
    tokio_mpsc::UnboundedReceiver<EncodeResult>,
) {
    let (job_tx, job_rx) = std::sync::mpsc::channel::<EncodeJob>();
    let (res_tx, res_rx) = tokio_mpsc::unbounded_channel::<EncodeResult>();

    std::thread::spawn(move || {
        let mut width = 0;
        let mut height = 0;
        let mut pts: i64 = 0;
        let mut force_kf_remaining: usize = 3; // 接続初期に数フレーム連続でキーフレームを送って復号を安定化
        let mut encoder: Option<Encoder<u8>> = None;

        while let Ok(job) = job_rx.recv() {
            // 最初のフレームまたは解像度変更時にエンコーダーを作成/再作成
            if encoder.is_none() || job.width != width || job.height != height {
                if encoder.is_some() {
                    info!(
                        "{} encoder: resizing encoder {}x{} -> {}x{}",
                        codec_name, width, height, job.width, job.height
                    );
                }
                width = job.width;
                height = job.height;
                match create_vpx_encoder(codec_id, width, height) {
                    Ok(enc) => {
                        encoder = Some(enc);
                        force_kf_remaining = 3; // リサイズ時も直後にキーフレームを送る
                    }
                    Err(e) => {
                        warn!("{} encoder: failed to create encoder: {}", codec_name, e);
                        continue;
                    }
                }
            }

            let encoder = encoder.as_mut().expect("encoder should be initialized");

            let rgb_start = Instant::now();
            let i420 = rgba_to_i420(&job.rgba, job.width, job.height);
            let rgb_dur = rgb_start.elapsed();

            let image = match YUVImageData::from_raw_data(
                ImageFormat::I420,
                job.width as usize,
                job.height as usize,
                &i420,
            ) {
                Ok(img) => img,
                Err(e) => {
                    warn!("{} encoder: invalid I420 buffer: {}", codec_name, e);
                    continue;
                }
            };

            let encode_start = Instant::now();
            let mut frame_flags = EncoderFrameFlags::empty();
            if force_kf_remaining > 0 {
                frame_flags |= EncoderFrameFlags::FORCE_KF;
                force_kf_remaining -= 1;
            }

            let packets = match encoder.encode(
                pts,
                job.duration.as_millis() as u64,
                image,
                EncodingDeadline::Realtime,
                frame_flags,
            ) {
                Ok(p) => p,
                Err(e) => {
                    warn!("{} encoder: encode failed: {}", codec_name, e);
                    continue;
                }
            };
            let encode_dur = encode_start.elapsed();
            pts = pts.saturating_add(job.duration.as_millis() as i64);

            for packet in packets {
                if let Packet::CompressedFrame(frame) = packet {
                    let sample_data = frame.data;
                    if sample_data.is_empty() {
                        continue;
                    }
                    let sample_size = sample_data.len();
                    let total_dur = job.enqueue_at.elapsed();

                    if res_tx
                        .send(EncodeResult {
                            sample_data,
                            is_keyframe: frame.flags.is_key,
                            duration: job.duration,
                            width: job.width,
                            height: job.height,
                            rgb_dur,
                            encode_dur,
                            pack_dur: Duration::from_millis(0),
                            total_dur,
                            sample_size,
                        })
                        .is_err()
                    {
                        return;
                    }
                }
            }
        }

        debug!("{} encoder worker exiting", codec_name);
    });

    (job_tx, res_rx)
}

/// VPXエンコードワーカーを複数起動し、結果を1つのチャネルに集約する
pub fn start_vpx_encode_workers(
    codec_id: CodecId,
    codec_name: &'static str,
    worker_count: usize,
) -> (
    Vec<std::sync::mpsc::Sender<EncodeJob>>,
    tokio_mpsc::UnboundedReceiver<EncodeResult>,
) {
    let worker_count = worker_count.max(1);
    let mut job_txs = Vec::with_capacity(worker_count);
    let (merged_tx, merged_rx) = tokio_mpsc::unbounded_channel::<EncodeResult>();

    for _ in 0..worker_count {
        let (job_tx, mut res_rx) = start_vpx_encode_worker(codec_id, codec_name);
        job_txs.push(job_tx);

        let merged_tx_clone = merged_tx.clone();
        tokio::spawn(async move {
            while let Some(result) = res_rx.recv().await {
                if merged_tx_clone.send(result).is_err() {
                    break;
                }
            }
        });
    }

    drop(merged_tx);
    (job_txs, merged_rx)
}

