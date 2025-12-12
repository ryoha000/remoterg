use core_types::{EncodeJob, EncodeResult, VideoCodec, VideoEncoderFactory};

#[cfg(feature = "h264")]
pub mod openh264 {
    use anyhow::Context;
    use openh264::encoder::{BitRate, EncoderConfig, FrameRate};
    use openh264::formats::RgbSliceU8;
    use openh264::OpenH264API;
    #[cfg(feature = "rayon")]
    use rayon::prelude::*;
    use std::time::{Duration, Instant};
    use tokio::sync::mpsc as tokio_mpsc;
    use tracing::{debug, info, warn};

    use super::{EncodeJob, EncodeResult, VideoCodec, VideoEncoderFactory};

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
            Vec<std::sync::mpsc::Sender<EncodeJob>>,
            tokio_mpsc::UnboundedReceiver<EncodeResult>,
        ) {
            start_encode_workers()
        }

        fn codec(&self) -> VideoCodec {
            VideoCodec::H264
        }
    }

    /// OpenH264のEncodedBitStreamからAnnex-B形式のH.264データを生成
    /// 戻り値: (Annex-B形式のデータ, SPS/PPSが含まれているか)
    fn annexb_from_bitstream(bitstream: &openh264::encoder::EncodedBitStream) -> (Vec<u8>, bool) {
        const START_CODE: &[u8] = &[0x00, 0x00, 0x00, 0x01];
        const START_CODE_SIZE: usize = 4;
        let mut has_sps_pps = false;

        let num_layers = bitstream.num_layers();
        if num_layers == 0 {
            warn!("EncodedBitStream has no layers");
            return (Vec::new(), has_sps_pps);
        }

        debug!("Processing {} layers", num_layers);

        // まず総サイズを推定してreserve（2パス化は避ける）
        let mut estimated_size = 0usize;
        for i in 0..num_layers {
            if let Some(layer) = bitstream.layer(i) {
                let nal_count = layer.nal_count();
                for j in 0..nal_count {
                    if let Some(nal_unit) = layer.nal_unit(j) {
                        if !nal_unit.is_empty() {
                            let has_start_code = nal_unit.len() >= 4
                                && nal_unit[0] == 0x00
                                && nal_unit[1] == 0x00
                                && nal_unit[2] == 0x00
                                && nal_unit[3] == 0x01;
                            estimated_size += nal_unit.len();
                            if !has_start_code {
                                estimated_size += START_CODE_SIZE;
                            }
                        }
                    }
                }
            }
        }

        let mut sample_data = Vec::with_capacity(estimated_size);

        // 実際のデータを構築
        for i in 0..num_layers {
            if let Some(layer) = bitstream.layer(i) {
                let nal_count = layer.nal_count();
                debug!("Layer {}: {} NAL units", i, nal_count);

                if nal_count == 0 {
                    warn!("Layer {} has no NAL units", i);
                    continue;
                }

                for j in 0..nal_count {
                    if let Some(nal_unit) = layer.nal_unit(j) {
                        if nal_unit.is_empty() {
                            warn!("NAL unit {} in layer {} is empty", j, i);
                            continue;
                        }

                        let has_start_code = nal_unit.len() >= 4
                            && nal_unit[0] == 0x00
                            && nal_unit[1] == 0x00
                            && nal_unit[2] == 0x00
                            && nal_unit[3] == 0x01;

                        let nal_header_offset = if has_start_code { 4 } else { 0 };

                        if nal_unit.len() <= nal_header_offset {
                            warn!(
                                "NAL unit {} in layer {} is too small ({} bytes, offset {})",
                                j,
                                i,
                                nal_unit.len(),
                                nal_header_offset
                            );
                            continue;
                        }

                        let nal_type = nal_unit[nal_header_offset] & 0x1F;
                        if nal_type == 7 || nal_type == 8 {
                            has_sps_pps = true;
                            info!(
                                "Found SPS/PPS: type={}, size={} bytes",
                                nal_type,
                                nal_unit.len()
                            );
                        }

                        if !has_start_code {
                            sample_data.extend_from_slice(START_CODE);
                        }

                        sample_data.extend_from_slice(nal_unit);
                    } else {
                        warn!("NAL unit {} in layer {} is None", j, i);
                    }
                }
            } else {
                warn!("Layer {} is None", i);
            }
        }

        debug!(
            "Total sample data: {} bytes (estimated: {}), has_sps_pps: {}",
            sample_data.len(),
            estimated_size,
            has_sps_pps
        );

        (sample_data, has_sps_pps)
    }

    /// OpenH264エンコードワーカーを生成
    fn start_encode_worker() -> (
        std::sync::mpsc::Sender<EncodeJob>,
        tokio_mpsc::UnboundedReceiver<EncodeResult>,
    ) {
        let (job_tx, job_rx) = std::sync::mpsc::channel::<EncodeJob>();
        let (res_tx, res_rx) = tokio_mpsc::unbounded_channel::<EncodeResult>();

        std::thread::spawn(move || {
            let mut width = 0;
            let mut height = 0;
            let mut encoder: Option<openh264::encoder::Encoder> = None;
            let mut encode_failures = 0u32;
            let mut empty_samples = 0u32;
            let mut successful_encodes = 0u32;
            let mut dropped_frames = 0u32;

            // パフォーマンス統計用
            let mut total_rgb_dur = Duration::ZERO;
            let mut total_encode_dur = Duration::ZERO;
            let mut total_pack_dur = Duration::ZERO;
            let mut total_queue_wait_dur = Duration::ZERO;
            let mut last_stats_log = Instant::now();

            // フロー制御: キューから複数のジョブを取得して、最新のものだけを処理
            // これにより、キューが溜まった場合でも低遅延を維持
            const MAX_QUEUE_DRAIN: usize = 10; // 一度にドレインする最大フレーム数

            loop {
                // 最初のジョブを取得（ブロッキング）
                let mut job = match job_rx.recv() {
                    Ok(job) => job,
                    Err(_) => break, // チャネルが閉じられた
                };

                // キューに溜まっている古いフレームをスキップして、最新のフレームを取得
                let mut skipped_count = 0;
                loop {
                    match job_rx.try_recv() {
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

                if skipped_count > 0 {
                    dropped_frames += skipped_count as u32;
                    if dropped_frames % 50 == 0 {
                        warn!(
                            "encoder worker: dropped {} frames due to queue backlog (low latency mode)",
                            dropped_frames
                        );
                    }
                }
                // キュー待ち時間を正確に計測: ジョブがキューに入ってからワーカーが受け取るまでの時間
                let recv_at = Instant::now();
                let queue_wait_dur = recv_at.duration_since(job.enqueue_at);
                // OpenH264は幅と高さが2の倍数である必要があるため、2の倍数に調整
                let encode_width = (job.width / 2) * 2;
                let encode_height = (job.height / 2) * 2;

                // 最初のフレームまたは解像度変更時にエンコーダーを作成/再作成
                if encoder.is_none() || encode_width != width || encode_height != height {
                    if encoder.is_some() {
                        info!(
                            "encoder worker: resizing encoder {}x{} -> {}x{} (original: {}x{})",
                            width, height, encode_width, encode_height, job.width, job.height
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

                let rgb_start = Instant::now();
                // 調整後の解像度に合わせてRGBデータを抽出
                let rgb_size = (encode_width * encode_height * 3) as usize;
                let mut rgb_data = vec![0u8; rgb_size];

                // RGBAからRGBへの変換（調整後の解像度分のみ）を並列化
                let rgba_src = &job.rgba;
                let src_width = job.width as usize;
                let dst_width = encode_width as usize;

                // 行単位で並列処理
                rgb_data
                    .par_chunks_mut(dst_width * 3)
                    .enumerate()
                    .for_each(|(y, rgb_row)| {
                        let src_row_start = y * src_width * 4;
                        let src_row_end = src_row_start + dst_width * 4;
                        if src_row_end <= rgba_src.len() {
                            let rgba_row = &rgba_src[src_row_start..src_row_end];
                            for (i, rgba_chunk) in rgba_row.chunks_exact(4).enumerate() {
                                let rgb_idx = i * 3;
                                rgb_row[rgb_idx] = rgba_chunk[0]; // R
                                rgb_row[rgb_idx + 1] = rgba_chunk[1]; // G
                                rgb_row[rgb_idx + 2] = rgba_chunk[2]; // B
                            }
                        }
                    });
                let rgb_dur = rgb_start.elapsed();

                // YUV変換のコストも計測に含める
                let yuv_start = Instant::now();
                let yuv = openh264::formats::YUVBuffer::from_rgb_source(RgbSliceU8::new(
                    &rgb_data,
                    (encode_width as usize, encode_height as usize),
                ));
                let yuv_dur = yuv_start.elapsed();
                // rgb_durにYUV変換時間も含める（色変換全体の時間として）
                let rgb_dur = rgb_dur + yuv_dur;

                let encode_start = Instant::now();
                match encoder.encode(&yuv) {
                    Ok(bitstream) => {
                        let encode_dur = encode_start.elapsed();
                        let pack_start = Instant::now();
                        let (sample_data, has_sps_pps) = annexb_from_bitstream(&bitstream);
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
                        total_rgb_dur += rgb_dur;
                        total_encode_dur += encode_dur;
                        total_pack_dur += pack_dur;
                        total_queue_wait_dur += queue_wait_dur;

                        // 50フレームごと、または5秒ごとに統計を出力
                        if successful_encodes % 50 == 0 || last_stats_log.elapsed().as_secs() >= 5 {
                            let avg_rgb = total_rgb_dur.as_secs_f64() / successful_encodes as f64;
                            let avg_encode =
                                total_encode_dur.as_secs_f64() / successful_encodes as f64;
                            let avg_pack = total_pack_dur.as_secs_f64() / successful_encodes as f64;
                            let avg_queue =
                                total_queue_wait_dur.as_secs_f64() / successful_encodes as f64;
                            info!(
                                "encoder worker stats [{} frames]: avg_rgb={:.3}ms avg_encode={:.3}ms avg_pack={:.3}ms avg_queue={:.3}ms",
                                successful_encodes,
                                avg_rgb * 1000.0,
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
                                rgb_dur,
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
                let avg_rgb = total_rgb_dur.as_secs_f64() / successful_encodes as f64;
                let avg_encode = total_encode_dur.as_secs_f64() / successful_encodes as f64;
                let avg_pack = total_pack_dur.as_secs_f64() / successful_encodes as f64;
                let avg_queue = total_queue_wait_dur.as_secs_f64() / successful_encodes as f64;
                let total_processing = total_rgb_dur + total_encode_dur + total_pack_dur;
                info!(
                    "encoder worker: final stats [{} frames]: total_rgb={:.3}s total_encode={:.3}s total_pack={:.3}s total_queue={:.3}s",
                    successful_encodes,
                    total_rgb_dur.as_secs_f64(),
                    total_encode_dur.as_secs_f64(),
                    total_pack_dur.as_secs_f64(),
                    total_queue_wait_dur.as_secs_f64()
                );
                info!(
                    "encoder worker: avg per frame: rgb={:.3}ms encode={:.3}ms pack={:.3}ms queue={:.3}ms total={:.3}ms",
                    avg_rgb * 1000.0,
                    avg_encode * 1000.0,
                    avg_pack * 1000.0,
                    avg_queue * 1000.0,
                    (avg_rgb + avg_encode + avg_pack) * 1000.0
                );
                if total_processing.as_secs_f64() > 0.0 {
                    info!(
                        "encoder worker: processing time distribution: rgb={:.1}% encode={:.1}% pack={:.1}%",
                        (total_rgb_dur.as_secs_f64() / total_processing.as_secs_f64()) * 100.0,
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
                "encoder worker: exiting (successful: {}, failures: {}, empty samples: {}, dropped frames: {})",
                successful_encodes, encode_failures, empty_samples, dropped_frames
            );
        });

        (job_tx, res_rx)
    }

    /// エンコードワーカーを複数起動し、結果を1つのチャネルに集約する
    pub fn start_encode_workers() -> (
        Vec<std::sync::mpsc::Sender<EncodeJob>>,
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
}

#[cfg(feature = "h264")]
#[path = "h264_mf.rs"]
pub mod h264_mf;

#[cfg(any(feature = "vp8", feature = "vp9"))]
mod vpx_common;

#[cfg(feature = "vp9")]
#[path = "vp9_vpx.rs"]
pub mod vp9;

#[cfg(feature = "vp8")]
#[path = "vp8_vpx.rs"]
pub mod vp8;
