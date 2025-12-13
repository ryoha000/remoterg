use anyhow::Context;
use core_types::{EncodeJob, EncodeResult, VideoCodec, VideoEncoderFactory};
use openh264::encoder::{BitRate, EncoderConfig, FrameRate};
use openh264::formats::YUVBuffer;
use openh264::OpenH264API;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc as tokio_mpsc;
use tracing::{info, warn};

use super::{annexb, rgba_to_yuv};

/// 前処理済みフレーム
struct PreprocessedFrame {
    seq: u64,
    yuv: YUVBuffer,
    encode_width: u32,
    encode_height: u32,
    duration: Duration,
    enqueue_at: Instant,
    preprocess_start: Instant, // 前処理ワーカーが受け取った時点
    preprocess_end: Instant,   // 前処理完了時点
    rgba_to_yuv_dur: Duration, // RGBA→YUV変換時間
    rgb_dur: Duration,         // 合計時間（後方互換性のため、rgba_to_yuv_durと同じ値）
}

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

/// RGBA→YUV変換を実行する前処理関数
fn preprocess_frame(
    job: EncodeJob,
    encode_width: u32,
    encode_height: u32,
) -> (YUVBuffer, Duration) {
    let rgba_to_yuv_start = Instant::now();

    // RGBA→YUV420変換
    let rgba_src = &job.rgba;
    let src_width = job.width as usize;
    let dst_width = encode_width as usize;
    let dst_height = encode_height as usize;

    let yuv_data = rgba_to_yuv::rgba_to_yuv420(rgba_src, dst_width, dst_height, src_width);

    let yuv = YUVBuffer::from_vec(yuv_data, dst_width, dst_height);
    let rgba_to_yuv_dur = rgba_to_yuv_start.elapsed();

    (yuv, rgba_to_yuv_dur)
}

/// 前処理ワーカー（RGBA→RGB→YUV変換を並列実行）
fn preprocess_worker(
    job_rx: Arc<Mutex<std::sync::mpsc::Receiver<(EncodeJob, u64, u32, u32)>>>,
    result_tx: std::sync::mpsc::Sender<PreprocessedFrame>,
) {
    loop {
        let preprocess_start = Instant::now();
        let (job, seq, encode_width, encode_height) = {
            let rx = job_rx.lock().unwrap();
            match rx.recv() {
                Ok(data) => data,
                Err(_) => break, // チャネルが閉じられた
            }
        };

        let duration = job.duration;
        let enqueue_at = job.enqueue_at;
        let (yuv, rgba_to_yuv_dur) = preprocess_frame(job, encode_width, encode_height);
        let preprocess_end = Instant::now();

        // 後方互換性のため、合計時間も保持（rgba_to_yuv_durと同じ値）
        let rgb_dur = rgba_to_yuv_dur;

        let preprocessed = PreprocessedFrame {
            seq,
            yuv,
            encode_width,
            encode_height,
            duration,
            enqueue_at,
            preprocess_start,
            preprocess_end,
            rgba_to_yuv_dur,
            rgb_dur,
        };

        if result_tx.send(preprocessed).is_err() {
            break;
        }
    }
}

/// OpenH264エンコードワーカーを生成（前処理並列化版）
fn start_encode_worker() -> (
    mpsc::Sender<EncodeJob>,
    tokio_mpsc::UnboundedReceiver<EncodeResult>,
) {
    let (job_tx, job_rx) = mpsc::channel::<EncodeJob>();
    let (res_tx, res_rx) = tokio_mpsc::unbounded_channel::<EncodeResult>();

    // 前処理ワーカーの数を決定（CPUコア数-1）
    let num_preprocess_workers = std::thread::available_parallelism()
        .map(|n| n.get().saturating_sub(1).max(1))
        .unwrap_or(1);

    info!(
        "Starting OpenH264 encoder with {} preprocess workers",
        num_preprocess_workers
    );

    // 前処理用のチャネル（std::sync::mpscを使用）
    let (preprocess_job_tx, preprocess_job_rx) =
        std::sync::mpsc::channel::<(EncodeJob, u64, u32, u32)>();
    let (preprocess_result_tx, preprocess_result_rx) =
        std::sync::mpsc::channel::<PreprocessedFrame>();

    // 前処理ワーカーを起動（ReceiverをArc<Mutex<>>で共有）
    let job_rx_shared = Arc::new(Mutex::new(preprocess_job_rx));
    for _ in 0..num_preprocess_workers {
        let job_rx = job_rx_shared.clone();
        let result_tx = preprocess_result_tx.clone();
        std::thread::spawn(move || preprocess_worker(job_rx, result_tx));
    }
    drop(preprocess_result_tx);

    // 入力スレッド: ジョブを受信してseqを付与し、前処理キューへ投入
    let seq_counter = AtomicU64::new(0);
    let input_job_rx = job_rx;
    let input_preprocess_tx = preprocess_job_tx.clone();
    std::thread::spawn(move || {
        const MAX_QUEUE_DRAIN: usize = 10; // 一度にドレインする最大フレーム数

        loop {
            // 最初のジョブを取得（ブロッキング）
            let mut job = match input_job_rx.recv() {
                Ok(job) => job,
                Err(_) => break, // チャネルが閉じられた
            };

            // キューに溜まっている古いフレームをスキップして、最新のフレームを取得
            let mut skipped_count = 0;
            loop {
                match input_job_rx.try_recv() {
                    Ok(newer_job) => {
                        // より新しいジョブが見つかった
                        if skipped_count < MAX_QUEUE_DRAIN {
                            skipped_count += 1;
                            job = newer_job;
                        } else {
                            // 最大ドレイン数に達したので、現在のジョブを処理
                            break;
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        // キューが空になったので、現在のジョブを処理
                        break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        // チャネルが閉じられた
                        return;
                    }
                }
            }

            // seqを付与
            let seq = seq_counter.fetch_add(1, Ordering::Relaxed);
            // OpenH264は幅と高さが2の倍数である必要があるため、2の倍数に調整
            let encode_width = (job.width / 2) * 2;
            let encode_height = (job.height / 2) * 2;

            // 前処理キューへ投入
            if input_preprocess_tx
                .send((job, seq, encode_width, encode_height))
                .is_err()
            {
                break;
            }
        }
    });
    drop(preprocess_job_tx);

    // エンコードスレッド: 前処理結果を順序通りにエンコード
    std::thread::spawn(move || {
        let mut width = 0;
        let mut height = 0;
        let mut encoder: Option<openh264::encoder::Encoder> = None;
        let mut encode_failures = 0u32;
        let mut empty_samples = 0u32;
        let mut successful_encodes = 0u32;

        // パフォーマンス統計用
        let mut total_rgba_to_yuv_dur = Duration::ZERO; // RGBA→YUV変換時間
        let mut total_rgb_dur = Duration::ZERO; // 合計時間（後方互換性のため）
        let mut total_encode_dur = Duration::ZERO;
        let mut total_pack_dur = Duration::ZERO;
        let mut total_queue_wait_dur = Duration::ZERO;
        let mut total_preprocess_queue_wait_dur = Duration::ZERO; // 前処理キュー待ち時間
        let mut total_result_queue_wait_dur = Duration::ZERO; // 前処理結果キュー待ち時間
        let mut last_stats_log = Instant::now();

        // 順序保証用のバッファ
        let mut buffer: BTreeMap<u64, PreprocessedFrame> = BTreeMap::new();
        let mut next_seq = 0u64;
        let mut channel_closed = false;

        loop {
            // 前処理結果を受信（非ブロッキングで試行、なければブロッキング）
            if !channel_closed {
                match preprocess_result_rx.try_recv() {
                    Ok(preprocessed) => {
                        buffer.insert(preprocessed.seq, preprocessed);
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        // キューが空なのでブロッキング受信を試行
                        match preprocess_result_rx.recv() {
                            Ok(preprocessed) => {
                                buffer.insert(preprocessed.seq, preprocessed);
                            }
                            Err(_) => {
                                // チャネルが閉じられた
                                channel_closed = true;
                            }
                        }
                    }
                    Err(_) => {
                        // チャネルが閉じられた
                        channel_closed = true;
                    }
                }
            }

            // 順序通りにエンコード
            while let Some(preprocessed) = buffer.remove(&next_seq) {
                next_seq += 1;

                let encode_width = preprocessed.encode_width;
                let encode_height = preprocessed.encode_height;

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

                // キュー待ち時間を計測（3つの要素に分離）
                let recv_at = Instant::now();
                // 1. 前処理キュー待ち時間: enqueue_atから前処理ワーカーが受け取るまで
                let preprocess_queue_wait_dur = preprocessed
                    .preprocess_start
                    .duration_since(preprocessed.enqueue_at);
                // 2. 前処理実行時間: rgb_dur（既に計測済み）
                // 3. 前処理結果キュー待ち時間: 前処理完了からエンコードスレッドが受け取るまで
                let result_queue_wait_dur = recv_at.duration_since(preprocessed.preprocess_end);
                // 全体のキュー待ち時間（後方互換性のため）
                let queue_wait_dur = recv_at.duration_since(preprocessed.enqueue_at);

                let encode_start = Instant::now();
                match encoder.encode(&preprocessed.yuv) {
                    Ok(bitstream) => {
                        let encode_dur = encode_start.elapsed();
                        let pack_start = Instant::now();
                        let (sample_data, has_sps_pps) = annexb::annexb_from_bitstream(&bitstream);
                        let pack_dur = pack_start.elapsed();

                        let sample_size = sample_data.len();
                        // 総処理時間: ジョブ作成からエンコード完了まで（キュー待ち + 処理時間）
                        let total_dur = recv_at.elapsed() + queue_wait_dur;

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
                        total_rgba_to_yuv_dur += preprocessed.rgba_to_yuv_dur;
                        total_rgb_dur += preprocessed.rgb_dur;
                        total_encode_dur += encode_dur;
                        total_pack_dur += pack_dur;
                        total_queue_wait_dur += queue_wait_dur;
                        total_preprocess_queue_wait_dur += preprocess_queue_wait_dur;
                        total_result_queue_wait_dur += result_queue_wait_dur;

                        // 50フレームごと、または5秒ごとに統計を出力
                        if successful_encodes % 50 == 0 || last_stats_log.elapsed().as_secs() >= 5 {
                            let avg_rgba_to_yuv =
                                total_rgba_to_yuv_dur.as_secs_f64() / successful_encodes as f64;
                            let avg_rgb = total_rgb_dur.as_secs_f64() / successful_encodes as f64;
                            let avg_encode =
                                total_encode_dur.as_secs_f64() / successful_encodes as f64;
                            let avg_pack = total_pack_dur.as_secs_f64() / successful_encodes as f64;
                            let avg_queue =
                                total_queue_wait_dur.as_secs_f64() / successful_encodes as f64;
                            let avg_preprocess_queue = total_preprocess_queue_wait_dur
                                .as_secs_f64()
                                / successful_encodes as f64;
                            let avg_result_queue = total_result_queue_wait_dur.as_secs_f64()
                                / successful_encodes as f64;
                            info!(
                                "encoder worker stats [{} frames]: avg_rgba_to_yuv={:.3}ms avg_rgb={:.3}ms avg_encode={:.3}ms avg_pack={:.3}ms avg_queue={:.3}ms (preprocess_queue={:.3}ms result_queue={:.3}ms)",
                                successful_encodes,
                                avg_rgba_to_yuv * 1000.0,
                                avg_rgb * 1000.0,
                                avg_encode * 1000.0,
                                avg_pack * 1000.0,
                                avg_queue * 1000.0,
                                avg_preprocess_queue * 1000.0,
                                avg_result_queue * 1000.0
                            );
                            last_stats_log = Instant::now();
                        }

                        if res_tx
                            .send(EncodeResult {
                                sample_data,
                                is_keyframe: has_sps_pps,
                                duration: preprocessed.duration,
                                width: encode_width,
                                height: encode_height,
                                rgb_dur: preprocessed.rgb_dur,
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

            // チャネルが閉じられてバッファが空になったら終了
            if channel_closed && buffer.is_empty() {
                break;
            }
        }

        // 最終統計を出力
        if successful_encodes > 0 {
            let avg_rgba_to_yuv = total_rgba_to_yuv_dur.as_secs_f64() / successful_encodes as f64;
            let avg_rgb = total_rgb_dur.as_secs_f64() / successful_encodes as f64;
            let avg_encode = total_encode_dur.as_secs_f64() / successful_encodes as f64;
            let avg_pack = total_pack_dur.as_secs_f64() / successful_encodes as f64;
            let avg_queue = total_queue_wait_dur.as_secs_f64() / successful_encodes as f64;
            let avg_preprocess_queue =
                total_preprocess_queue_wait_dur.as_secs_f64() / successful_encodes as f64;
            let avg_result_queue =
                total_result_queue_wait_dur.as_secs_f64() / successful_encodes as f64;
            let total_processing = total_rgb_dur + total_encode_dur + total_pack_dur;
            info!(
                "encoder worker: final stats [{} frames]: total_rgba_to_yuv={:.3}s total_rgb={:.3}s total_encode={:.3}s total_pack={:.3}s total_queue={:.3}s (preprocess_queue={:.3}s result_queue={:.3}s)",
                successful_encodes,
                total_rgba_to_yuv_dur.as_secs_f64(),
                total_rgb_dur.as_secs_f64(),
                total_encode_dur.as_secs_f64(),
                total_pack_dur.as_secs_f64(),
                total_queue_wait_dur.as_secs_f64(),
                total_preprocess_queue_wait_dur.as_secs_f64(),
                total_result_queue_wait_dur.as_secs_f64()
            );
            info!(
                "encoder worker: avg per frame: rgba_to_yuv={:.3}ms rgb={:.3}ms encode={:.3}ms pack={:.3}ms queue={:.3}ms (preprocess_queue={:.3}ms result_queue={:.3}ms) total={:.3}ms",
                avg_rgba_to_yuv * 1000.0,
                avg_rgb * 1000.0,
                avg_encode * 1000.0,
                avg_pack * 1000.0,
                avg_queue * 1000.0,
                avg_preprocess_queue * 1000.0,
                avg_result_queue * 1000.0,
                (avg_rgb + avg_encode + avg_pack) * 1000.0
            );
            if total_processing.as_secs_f64() > 0.0 {
                let rgba_to_yuv_pct =
                    (total_rgba_to_yuv_dur.as_secs_f64() / total_processing.as_secs_f64()) * 100.0;
                let rgb_pct =
                    (total_rgb_dur.as_secs_f64() / total_processing.as_secs_f64()) * 100.0;
                info!(
                    "encoder worker: processing time distribution: rgba_to_yuv={:.1}% rgb={:.1}% encode={:.1}% pack={:.1}%",
                    rgba_to_yuv_pct,
                    rgb_pct,
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
