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
///
/// # Arguments
/// * `rgba` - RGBA画像データ（元のサイズ）
/// * `source_width` - 元のRGBAデータの幅
/// * `source_height` - 元のRGBAデータの高さ
///
/// 奇数ピクセルの場合、左端または上端の1ピクセルが削除されます。
/// エンコード用のサイズは2の倍数に自動的に丸められます。
pub fn rgba_to_i420(
    rgba: &[u8],
    source_width: u32,
    source_height: u32,
) -> anyhow::Result<(Vec<u8>, u32, u32)> {
    let source_w = source_width as usize;
    let source_h = source_height as usize;

    // I420形式では幅と高さが2の倍数である必要があるため、2の倍数に丸める
    // 奇数ピクセルの場合、左端または上端の1ピクセルを削除
    let encode_w = (source_w / 2) * 2;
    let encode_h = (source_h / 2) * 2;

    let expected_size = source_w * source_h * 4;

    // RGBAデータのサイズを検証
    if rgba.len() < expected_size {
        return Err(anyhow::anyhow!(
            "RGBAデータのサイズが不足しています: 期待値 {} bytes, 実際 {} bytes (解像度: {}x{})",
            expected_size,
            rgba.len(),
            source_width,
            source_height
        ));
    }

    // 左端または上端のオフセットを計算（奇数ピクセルの場合、左端または上端の1ピクセルを削除）
    // 幅が奇数の場合、左端の1ピクセルを削除
    let offset_x = if source_w % 2 != 0 { 1 } else { 0 };
    // 高さが奇数の場合、上端の1ピクセルを削除
    let offset_y = if source_h % 2 != 0 { 1 } else { 0 };

    let y_plane = encode_w * encode_h;
    let uv_plane = y_plane / 4;

    let mut y = vec![0u8; y_plane];
    let mut u = vec![0u8; uv_plane];
    let mut v = vec![0u8; uv_plane];

    // Y平面の変換（左端または上端のオフセットを考慮）
    for j in 0..encode_h {
        for i in 0..encode_w {
            let source_row = j + offset_y;
            let source_col = i + offset_x;
            let source_idx = (source_row * source_w + source_col) * 4;
            if source_idx + 3 >= rgba.len() {
                return Err(anyhow::anyhow!(
                    "RGBAインデックス範囲外: idx={}, len={} (j={}, i={}, encode_w={}, encode_h={}, source_w={}, source_h={}, offset_x={}, offset_y={})",
                    source_idx, rgba.len(), j, i, encode_w, encode_h, source_w, source_h, offset_x, offset_y
                ));
            }
            let r = rgba[source_idx] as f32;
            let g = rgba[source_idx + 1] as f32;
            let b = rgba[source_idx + 2] as f32;

            let y_val = (0.257 * r + 0.504 * g + 0.098 * b + 16.0).round();
            y[j * encode_w + i] = y_val.clamp(0.0, 255.0) as u8;
        }
    }

    // UV平面の変換（左端または上端のオフセットを考慮）
    for j in (0..encode_h).step_by(2) {
        for i in (0..encode_w).step_by(2) {
            let mut u_acc = 0f32;
            let mut v_acc = 0f32;
            let mut sample_count = 0;
            for sj in 0..2 {
                let row = j + sj;
                if row >= encode_h {
                    continue;
                }
                for si in 0..2 {
                    let col = i + si;
                    if col >= encode_w {
                        continue;
                    }
                    let source_row = row + offset_y;
                    let source_col = col + offset_x;
                    let source_idx = (source_row * source_w + source_col) * 4;
                    if source_idx + 3 >= rgba.len() {
                        return Err(anyhow::anyhow!(
                            "RGBAインデックス範囲外: idx={}, len={} (j={}, i={}, sj={}, si={}, encode_w={}, encode_h={}, source_w={}, source_h={}, offset_x={}, offset_y={})",
                            source_idx, rgba.len(), j, i, sj, si, encode_w, encode_h, source_w, source_h, offset_x, offset_y
                        ));
                    }
                    let r = rgba[source_idx] as f32;
                    let g = rgba[source_idx + 1] as f32;
                    let b = rgba[source_idx + 2] as f32;
                    u_acc += -0.148 * r - 0.291 * g + 0.439 * b + 128.0;
                    v_acc += 0.439 * r - 0.368 * g - 0.071 * b + 128.0;
                    sample_count += 1;
                }
            }
            let uv_idx = (j / 2) * (encode_w / 2) + (i / 2);
            if uv_idx < uv_plane {
                if sample_count > 0 {
                    u[uv_idx] = (u_acc / sample_count as f32).clamp(0.0, 255.0) as u8;
                    v[uv_idx] = (v_acc / sample_count as f32).clamp(0.0, 255.0) as u8;
                }
            }
        }
    }

    let mut buffer = Vec::with_capacity(y_plane + 2 * uv_plane);
    buffer.extend_from_slice(&y);
    buffer.extend_from_slice(&u);
    buffer.extend_from_slice(&v);
    Ok((buffer, encode_w as u32, encode_h as u32))
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
            let rgb_start = Instant::now();
            // RGBAデータから必要な部分だけを切り出してI420に変換
            let (i420, actual_encode_width, actual_encode_height) =
                match rgba_to_i420(&job.rgba, job.width, job.height) {
                    Ok(result) => result,
                    Err(e) => {
                        warn!(
                            "{} encoder: failed to convert RGBA to I420: {}",
                            codec_name, e
                        );
                        eprintln!(
                            "{} encoder: failed to convert RGBA to I420: {}",
                            codec_name, e
                        );
                        eprintln!(
                            "  RGBA size: {} bytes, expected: {} bytes",
                            job.rgba.len(),
                            job.width as usize * job.height as usize * 4
                        );
                        continue;
                    }
                };
            let rgb_dur = rgb_start.elapsed();

            // 最初のフレームまたは解像度変更時にエンコーダーを作成/再作成
            if encoder.is_none() || actual_encode_width != width || actual_encode_height != height {
                if encoder.is_some() {
                    info!(
                        "{} encoder: resizing encoder {}x{} -> {}x{}",
                        codec_name, width, height, actual_encode_width, actual_encode_height
                    );
                }
                width = actual_encode_width;
                height = actual_encode_height;
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

            let image = match YUVImageData::from_raw_data(
                ImageFormat::I420,
                actual_encode_width as usize,
                actual_encode_height as usize,
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
                            width: actual_encode_width,
                            height: actual_encode_height,
                            rgb_dur,
                            encode_dur,
                            pack_dur: Duration::from_millis(0),
                            total_dur,
                            sample_size,
                        })
                        .is_err()
                    {
                        eprintln!(
                            "{} encoder: failed to send result, receiver closed",
                            codec_name
                        );
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
    _worker_count: usize,
) -> (
    Vec<std::sync::mpsc::Sender<EncodeJob>>,
    tokio_mpsc::UnboundedReceiver<EncodeResult>,
) {
    // encoderの整合性を保つため、常に1つのワーカーのみを起動
    // Pフレームが適切に参照フレームを参照できるようにする
    let (job_tx, res_rx) = start_vpx_encode_worker(codec_id, codec_name);
    (vec![job_tx], res_rx)
}
