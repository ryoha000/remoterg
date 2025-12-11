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
        ) -> (
            Vec<std::sync::mpsc::Sender<EncodeJob>>,
            tokio_mpsc::UnboundedReceiver<EncodeResult>,
        ) {
            start_encode_workers(worker_count)
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

            while let Ok(job) = job_rx.recv() {
                // 最初のフレームまたは解像度変更時にエンコーダーを作成/再作成
                if encoder.is_none() || job.width != width || job.height != height {
                    if encoder.is_some() {
                        info!(
                            "encoder worker: resizing encoder {}x{} -> {}x{}",
                            width, height, job.width, job.height
                        );
                    }
                    width = job.width;
                    height = job.height;
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
    ) -> (
        Vec<std::sync::mpsc::Sender<EncodeJob>>,
        tokio_mpsc::UnboundedReceiver<EncodeResult>,
    ) {
        let worker_count = worker_count.max(1);
        let mut job_txs = Vec::with_capacity(worker_count);
        let (merged_tx, merged_rx) = tokio_mpsc::unbounded_channel::<EncodeResult>();

        for _ in 0..worker_count {
            let (job_tx, mut res_rx) = start_encode_worker();
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

#[cfg(any(feature = "vp8", feature = "vp9"))]
mod vpx_common;

#[cfg(feature = "vp9")]
#[path = "vp9_vpx.rs"]
pub mod vp9;

#[cfg(feature = "vp8")]
#[path = "vp8_vpx.rs"]
pub mod vp8;
