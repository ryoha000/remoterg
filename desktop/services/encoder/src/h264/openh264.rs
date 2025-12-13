use anyhow::Context;
use core_types::{EncodeJob, EncodeResult, VideoCodec, VideoEncoderFactory};
use openh264::encoder::{BitRate, EncoderConfig, FrameRate};
use openh264::formats::YUVBuffer;
use openh264::OpenH264API;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc as tokio_mpsc;
use tracing::{info, warn};

use super::{annexb, rgba_to_yuv};

/// OpenH264 ファクトリ
pub struct OpenH264EncoderFactory;

impl OpenH264EncoderFactory {
    pub fn new() -> Self {
        Self
    }
}

impl VideoEncoderFactory for OpenH264EncoderFactory {
    fn start_workers(
        &self,
    ) -> (
        Vec<mpsc::Sender<EncodeJob>>,
        tokio_mpsc::UnboundedReceiver<EncodeResult>,
    ) {
        start_encode_workers()
    }

    fn codec(&self) -> VideoCodec {
        VideoCodec::H264
    }
}

/// OpenH264エンコードワーカーを生成（前処理→エンコードを直列実行）
fn start_encode_worker() -> (
    mpsc::Sender<EncodeJob>,
    tokio_mpsc::UnboundedReceiver<EncodeResult>,
) {
    let (job_tx, job_rx) = mpsc::channel::<EncodeJob>();
    let (res_tx, res_rx) = tokio_mpsc::unbounded_channel::<EncodeResult>();

    info!("Starting OpenH264 encoder with serial preprocessing");

    // エンコードスレッド: ジョブを受信→前処理→エンコードを直列実行
    std::thread::spawn(move || {
        let mut width = 0;
        let mut height = 0;
        let mut encoder: Option<openh264::encoder::Encoder> = None;
        let mut encode_failures = 0u32;
        let mut empty_samples = 0u32;
        let mut successful_encodes = 0u32;

        // パフォーマンス統計用
        let mut total_rgba_to_yuv_dur = Duration::ZERO;
        let mut total_encode_dur = Duration::ZERO;
        let mut total_pack_dur = Duration::ZERO;
        let mut total_queue_wait_dur = Duration::ZERO;
        let mut last_stats_log = Instant::now();

        const MAX_QUEUE_DRAIN: usize = 10; // 一度にドレインする最大フレーム数

        loop {
            // ジョブを受信（キューに溜まっている古いフレームをスキップ）
            let mut job = match job_rx.recv() {
                Ok(job) => job,
                Err(_) => break, // チャネルが閉じられた
            };

            // キューに溜まっている古いフレームをスキップして、最新のフレームを取得
            let mut skipped_count = 0;
            loop {
                match job_rx.try_recv() {
                    Ok(newer_job) => {
                        if skipped_count < MAX_QUEUE_DRAIN {
                            skipped_count += 1;
                            job = newer_job;
                        } else {
                            break;
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        return;
                    }
                }
            }

            let process_start = Instant::now();
            let queue_wait_dur = process_start.duration_since(job.enqueue_at);

            // OpenH264は幅と高さが2の倍数である必要があるため、2の倍数に調整
            let encode_width = (job.width / 2) * 2;
            let encode_height = (job.height / 2) * 2;

            // 前処理: RGBA→YUV変換
            let rgba_to_yuv_start = Instant::now();
            let rgba_src = &job.rgba;
            let src_width = job.width as usize;
            let dst_width = encode_width as usize;
            let dst_height = encode_height as usize;

            let yuv_data = rgba_to_yuv::rgba_to_yuv420(rgba_src, dst_width, dst_height, src_width);
            let yuv = YUVBuffer::from_vec(yuv_data, dst_width, dst_height);
            let rgba_to_yuv_dur = rgba_to_yuv_start.elapsed();

            // 最初のフレームまたは解像度変更時にエンコーダーを作成/再作成
            if encoder.is_none() || encode_width != width || encode_height != height {
                if encoder.is_some() {
                    info!(
                        "encoder worker: resizing encoder {}x{} -> {}x{}",
                        width, height, encode_width, encode_height
                    );
                }
                width = encode_width;
                height = encode_height;
                match create_encoder(width, height) {
                    Ok(enc) => encoder = Some(enc),
                    Err(e) => {
                        warn!("encoder worker: failed to create encoder: {}", e);
                        continue;
                    }
                }
            }

            let encoder = encoder.as_mut().expect("encoder should be initialized");

            // エンコード
            let encode_start = Instant::now();
            match encoder.encode(&yuv) {
                Ok(bitstream) => {
                    let encode_dur = encode_start.elapsed();
                    let pack_start = Instant::now();
                    let (sample_data, has_sps_pps) = annexb::annexb_from_bitstream(&bitstream);
                    let pack_dur = pack_start.elapsed();

                    let sample_size = sample_data.len();
                    let total_dur = process_start.elapsed();

                    if sample_size == 0 {
                        empty_samples += 1;
                        warn!(
                            "encoder worker: empty sample, skipping (total empty: {})",
                            empty_samples
                        );
                        continue;
                    }

                    successful_encodes += 1;

                    // 統計を累積
                    total_rgba_to_yuv_dur += rgba_to_yuv_dur;
                    total_encode_dur += encode_dur;
                    total_pack_dur += pack_dur;
                    total_queue_wait_dur += queue_wait_dur;

                    // 50フレームごと、または5秒ごとに統計を出力
                    if successful_encodes % 50 == 0 || last_stats_log.elapsed().as_secs() >= 5 {
                        let avg_rgba_to_yuv =
                            total_rgba_to_yuv_dur.as_secs_f64() / successful_encodes as f64;
                        let avg_encode = total_encode_dur.as_secs_f64() / successful_encodes as f64;
                        let avg_pack = total_pack_dur.as_secs_f64() / successful_encodes as f64;
                        let avg_queue =
                            total_queue_wait_dur.as_secs_f64() / successful_encodes as f64;
                        info!(
                            "encoder worker stats [{} frames]: avg_rgba_to_yuv={:.3}ms avg_encode={:.3}ms avg_pack={:.3}ms avg_queue={:.3}ms",
                            successful_encodes,
                            avg_rgba_to_yuv * 1000.0,
                            avg_encode * 1000.0,
                            avg_pack * 1000.0,
                            avg_queue * 1000.0
                        );
                        last_stats_log = Instant::now();
                    }

                    if res_tx
                        .send(EncodeResult {
                            sample_data,
                            is_keyframe: has_sps_pps,
                            duration: job.duration,
                            width: encode_width,
                            height: encode_height,
                            rgb_dur: rgba_to_yuv_dur,
                            encode_dur,
                            pack_dur,
                            total_dur,
                            sample_size,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                Err(e) => {
                    encode_failures += 1;
                    warn!(
                        "encoder worker: encode failed: {} (total failures: {})",
                        e, encode_failures
                    );
                }
            }
        }

        // 最終統計を出力
        if successful_encodes > 0 {
            let avg_rgba_to_yuv = total_rgba_to_yuv_dur.as_secs_f64() / successful_encodes as f64;
            let avg_encode = total_encode_dur.as_secs_f64() / successful_encodes as f64;
            let avg_pack = total_pack_dur.as_secs_f64() / successful_encodes as f64;
            let avg_queue = total_queue_wait_dur.as_secs_f64() / successful_encodes as f64;
            let total_processing = total_rgba_to_yuv_dur + total_encode_dur + total_pack_dur;
            info!(
                "encoder worker: final stats [{} frames]: total_rgba_to_yuv={:.3}s total_encode={:.3}s total_pack={:.3}s total_queue={:.3}s",
                successful_encodes,
                total_rgba_to_yuv_dur.as_secs_f64(),
                total_encode_dur.as_secs_f64(),
                total_pack_dur.as_secs_f64(),
                total_queue_wait_dur.as_secs_f64()
            );
            info!(
                "encoder worker: avg per frame: rgba_to_yuv={:.3}ms encode={:.3}ms pack={:.3}ms queue={:.3}ms total={:.3}ms",
                avg_rgba_to_yuv * 1000.0,
                avg_encode * 1000.0,
                avg_pack * 1000.0,
                avg_queue * 1000.0,
                (avg_rgba_to_yuv + avg_encode + avg_pack) * 1000.0
            );
            if total_processing.as_secs_f64() > 0.0 {
                let rgba_to_yuv_pct =
                    (total_rgba_to_yuv_dur.as_secs_f64() / total_processing.as_secs_f64()) * 100.0;
                info!(
                    "encoder worker: processing time distribution: rgba_to_yuv={:.1}% encode={:.1}% pack={:.1}%",
                    rgba_to_yuv_pct,
                    (total_encode_dur.as_secs_f64() / total_processing.as_secs_f64()) * 100.0,
                    (total_pack_dur.as_secs_f64() / total_processing.as_secs_f64()) * 100.0
                );
            }
            let total_wall_time = total_queue_wait_dur + total_processing;
            if total_wall_time.as_secs_f64() > 0.0 {
                info!(
                    "encoder worker: wall time distribution: queue_wait={:.1}% processing={:.1}%",
                    (total_queue_wait_dur.as_secs_f64() / total_wall_time.as_secs_f64()) * 100.0,
                    (total_processing.as_secs_f64() / total_wall_time.as_secs_f64()) * 100.0
                );
            }
        }

        info!(
            "encoder worker: exiting (successful: {}, failures: {}, empty samples: {})",
            successful_encodes, encode_failures, empty_samples
        );
    });

    (job_tx, res_rx)
}

/// エンコードワーカーを複数起動し、結果を1つのチャネルに集約する
pub fn start_encode_workers() -> (
    Vec<mpsc::Sender<EncodeJob>>,
    tokio_mpsc::UnboundedReceiver<EncodeResult>,
) {
    // encoderの整合性を保つため、常に1つのワーカーのみを起動
    // Pフレームが適切に参照フレームを参照できるようにする
    let (job_tx, res_rx) = start_encode_worker();
    (vec![job_tx], res_rx)
}

fn create_encoder(width: u32, height: u32) -> anyhow::Result<openh264::encoder::Encoder> {
    let bitrate = (width * height * 2) as u32;
    // スレッド数はCPUコア数に合わせて調整（最大16スレッド）
    let num_threads = std::thread::available_parallelism()
        .map(|n| n.get().min(16) as u16)
        .unwrap_or(4);
    let encoder_config = EncoderConfig::new()
        .bitrate(BitRate::from_bps(bitrate))
        .max_frame_rate(FrameRate::from_hz(60.0))
        // skip_framesをfalseにして、できるだけすべてのフレームをエンコード
        // 実運用では、フレームをスキップせずにエンコードする方が品質が良い
        .skip_frames(false)
        .num_threads(num_threads);
    openh264::encoder::Encoder::with_api_config(OpenH264API::from_source(), encoder_config)
        .context("Failed to create OpenH264 encoder")
}
