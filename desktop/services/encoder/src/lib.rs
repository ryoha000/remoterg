use core_types::{EncodeJob, EncodeResult, VideoCodec, VideoEncoderFactory};

#[cfg(feature = "h264")]
pub mod openh264 {
    use anyhow::Context;
    use openh264::encoder::{BitRate, EncoderConfig, FrameRate};
    use openh264::formats::RgbSliceU8;
    use openh264::OpenH264API;
    use std::time::Instant;
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
            worker_count: usize,
            init_width: u32,
            init_height: u32,
        ) -> (
            Vec<std::sync::mpsc::Sender<EncodeJob>>,
            tokio_mpsc::UnboundedReceiver<EncodeResult>,
        ) {
            start_encode_workers(worker_count, init_width, init_height)
        }

        fn codec(&self) -> VideoCodec {
            VideoCodec::H264
        }
    }

    /// OpenH264のEncodedBitStreamからAnnex-B形式のH.264データを生成
    /// 戻り値: (Annex-B形式のデータ, SPS/PPSが含まれているか)
    fn annexb_from_bitstream(bitstream: &openh264::encoder::EncodedBitStream) -> (Vec<u8>, bool) {
        const START_CODE: &[u8] = &[0x00, 0x00, 0x00, 0x01];
        let mut sample_data = Vec::new();
        let mut has_sps_pps = false;

        let num_layers = bitstream.num_layers();
        if num_layers == 0 {
            warn!("EncodedBitStream has no layers");
            return (sample_data, has_sps_pps);
        }

        debug!("Processing {} layers", num_layers);

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
            "Total sample data: {} bytes, has_sps_pps: {}",
            sample_data.len(),
            has_sps_pps
        );

        (sample_data, has_sps_pps)
    }

    /// OpenH264エンコードワーカーを生成
    fn start_encode_worker(
        init_width: u32,
        init_height: u32,
    ) -> (
        std::sync::mpsc::Sender<EncodeJob>,
        tokio_mpsc::UnboundedReceiver<EncodeResult>,
    ) {
        let (job_tx, job_rx) = std::sync::mpsc::channel::<EncodeJob>();
        let (res_tx, res_rx) = tokio_mpsc::unbounded_channel::<EncodeResult>();

        std::thread::spawn(move || {
            let mut width = init_width;
            let mut height = init_height;
            let mut encoder = create_encoder(width, height).expect("failed to create encoder");

            while let Ok(job) = job_rx.recv() {
                if job.width != width || job.height != height {
                    info!(
                        "encoder worker: resizing encoder {}x{} -> {}x{}",
                        width, height, job.width, job.height
                    );
                    width = job.width;
                    height = job.height;
                    match create_encoder(width, height) {
                        Ok(enc) => encoder = enc,
                        Err(e) => {
                            warn!("encoder worker: failed to recreate encoder: {}", e);
                            continue;
                        }
                    }
                }

                let rgb_start = Instant::now();
                let rgb_size = (job.width * job.height * 3) as usize;
                let mut rgb_data = Vec::with_capacity(rgb_size);
                for i in 0..(job.width * job.height) as usize {
                    let rgba_idx = i * 4;
                    rgb_data.push(job.rgba[rgba_idx]); // R
                    rgb_data.push(job.rgba[rgba_idx + 1]); // G
                    rgb_data.push(job.rgba[rgba_idx + 2]); // B
                }
                let rgb_dur = rgb_start.elapsed();

                let yuv = openh264::formats::YUVBuffer::from_rgb_source(RgbSliceU8::new(
                    &rgb_data,
                    (job.width as usize, job.height as usize),
                ));

                let encode_start = Instant::now();
                match encoder.encode(&yuv) {
                    Ok(bitstream) => {
                        let encode_dur = encode_start.elapsed();
                        let pack_start = Instant::now();
                        let (sample_data, has_sps_pps) = annexb_from_bitstream(&bitstream);
                        let pack_dur = pack_start.elapsed();

                        let sample_size = sample_data.len();
                        let total_dur = job.enqueue_at.elapsed();

                        if sample_size == 0 {
                            warn!("encoder worker: empty sample, skipping");
                            continue;
                        }

                        if res_tx
                            .send(EncodeResult {
                                sample_data,
                                is_keyframe: has_sps_pps,
                                duration: job.duration,
                                width: job.width,
                                height: job.height,
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
                        warn!("encoder worker: encode failed: {}", e);
                    }
                }
            }

            debug!("encoder worker: exiting");
        });

        (job_tx, res_rx)
    }

    /// エンコードワーカーを複数起動し、結果を1つのチャネルに集約する
    fn start_encode_workers(
        worker_count: usize,
        init_width: u32,
        init_height: u32,
    ) -> (
        Vec<std::sync::mpsc::Sender<EncodeJob>>,
        tokio_mpsc::UnboundedReceiver<EncodeResult>,
    ) {
        let worker_count = worker_count.max(1);
        let mut job_txs = Vec::with_capacity(worker_count);
        let (merged_tx, merged_rx) = tokio_mpsc::unbounded_channel::<EncodeResult>();

        for _ in 0..worker_count {
            let (job_tx, mut res_rx) = start_encode_worker(init_width, init_height);
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

    fn create_encoder(width: u32, height: u32) -> anyhow::Result<openh264::encoder::Encoder> {
        let bitrate = (width * height * 2) as u32;
        let encoder_config = EncoderConfig::new()
            .bitrate(BitRate::from_bps(bitrate))
            .max_frame_rate(FrameRate::from_hz(60.0))
            .skip_frames(false);
        openh264::encoder::Encoder::with_api_config(OpenH264API::from_source(), encoder_config)
            .context("Failed to create OpenH264 encoder")
    }
}

#[cfg(feature = "vp9")]
pub mod vp9 {
    use std::time::{Duration, Instant};
    use tokio::sync::mpsc as tokio_mpsc;
    use tracing::{debug, warn};
    use vpx_encode::{Config, Encoder, VideoCodecId};

    use super::{EncodeJob, EncodeResult, VideoCodec, VideoEncoderFactory};

    /// VP9 ファクトリ
    pub struct Vp9EncoderFactory;

    impl Vp9EncoderFactory {
        pub fn new() -> Self {
            Self
        }
    }

    impl VideoEncoderFactory for Vp9EncoderFactory {
        fn start_workers(
            &self,
            worker_count: usize,
            init_width: u32,
            init_height: u32,
        ) -> (
            Vec<std::sync::mpsc::Sender<EncodeJob>>,
            tokio_mpsc::UnboundedReceiver<EncodeResult>,
        ) {
            start_encode_workers(worker_count, init_width, init_height)
        }

        fn codec(&self) -> VideoCodec {
            VideoCodec::Vp9
        }
    }

    fn rgba_to_i420(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
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
                        u_acc += (-0.148 * r - 0.291 * g + 0.439 * b + 128.0);
                        v_acc += (0.439 * r - 0.368 * g - 0.071 * b + 128.0);
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

    fn start_encode_worker(
        init_width: u32,
        init_height: u32,
    ) -> (
        std::sync::mpsc::Sender<EncodeJob>,
        tokio_mpsc::UnboundedReceiver<EncodeResult>,
    ) {
        let (job_tx, job_rx) = std::sync::mpsc::channel::<EncodeJob>();
        let (res_tx, res_rx) = tokio_mpsc::unbounded_channel::<EncodeResult>();

        std::thread::spawn(move || {
            let mut width = init_width;
            let mut height = init_height;
            let mut pts: i64 = 0;

            let mut encoder = create_encoder(width, height).expect("failed to create vp9 encoder");

            while let Ok(job) = job_rx.recv() {
                if job.width != width || job.height != height {
                    width = job.width;
                    height = job.height;
                    match create_encoder(width, height) {
                        Ok(enc) => encoder = enc,
                        Err(e) => {
                            warn!("vp9 encoder recreate failed: {}", e);
                            continue;
                        }
                    }
                }

                let rgb_start = Instant::now();
                let i420 = rgba_to_i420(&job.rgba, job.width, job.height);
                let rgb_dur = rgb_start.elapsed();

                let encode_start = Instant::now();
                let packets = match encoder.encode(pts, &i420) {
                    Ok(p) => p,
                    Err(e) => {
                        warn!("vp9 encode failed: {}", e);
                        continue;
                    }
                };
                let encode_dur = encode_start.elapsed();
                pts = pts.saturating_add(job.duration.as_millis() as i64);

                for frame in packets {
                    let sample_data = frame.data.to_vec();
                    if sample_data.is_empty() {
                        continue;
                    }
                    let sample_size = sample_data.len();
                    let total_dur = job.enqueue_at.elapsed();

                    if res_tx
                        .send(EncodeResult {
                            sample_data,
                            is_keyframe: frame.key,
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

            debug!("vp9 encoder worker exiting");
        });

        (job_tx, res_rx)
    }

    fn start_encode_workers(
        worker_count: usize,
        init_width: u32,
        init_height: u32,
    ) -> (
        Vec<std::sync::mpsc::Sender<EncodeJob>>,
        tokio_mpsc::UnboundedReceiver<EncodeResult>,
    ) {
        let worker_count = worker_count.max(1);
        let mut job_txs = Vec::with_capacity(worker_count);
        let (merged_tx, merged_rx) = tokio_mpsc::unbounded_channel::<EncodeResult>();

        for _ in 0..worker_count {
            let (job_tx, mut res_rx) = start_encode_worker(init_width, init_height);
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

    fn create_encoder(width: u32, height: u32) -> anyhow::Result<Encoder> {
        let bitrate_kbps = ((width as u64 * height as u64 * 2) / 1000).max(300) as u32;
        let config = Config {
            width: width as _,
            height: height as _,
            timebase: [1, 1000],
            bitrate: bitrate_kbps,
            codec: VideoCodecId::VP9,
        };
        Encoder::new(config).map_err(|e| anyhow::anyhow!(format!("{e}")))
    }
}
