use anyhow::Context;
use core_types::{EncodeJobQueue, EncodeResult, VideoCodec, VideoEncoderFactory};
use openh264::encoder::{BitRate, EncoderConfig, FrameRate, RateControlMode};
use openh264::formats::YUVBuffer;
use openh264::OpenH264API;
use std::sync::Arc;
use tokio::sync::mpsc as tokio_mpsc;
use tracing::{info, span, warn, Level};

use super::{annexb, rgba_to_yuv};

/// OpenH264 ファクトリ
pub struct OpenH264EncoderFactory;

impl OpenH264EncoderFactory {
    pub fn new() -> Self {
        Self
    }
}

impl VideoEncoderFactory for OpenH264EncoderFactory {
    fn setup(
        &self,
    ) -> (
        Arc<EncodeJobQueue>,
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
    Arc<EncodeJobQueue>,
    tokio_mpsc::UnboundedReceiver<EncodeResult>,
) {
    let job_queue = EncodeJobQueue::new();
    let job_queue_clone = Arc::clone(&job_queue);
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

        loop {
            // ジョブを取得（ブロッキング、最新のフレームのみ）
            let job = job_queue_clone.take();

            // OpenH264は幅と高さが2の倍数である必要があるため、2の倍数に調整
            let encode_width = (job.width / 2) * 2;
            let encode_height = (job.height / 2) * 2;

            // エンコードフレーム処理全体を span で計測
            let encode_frame_span = span!(
                Level::DEBUG,
                "encode_frame",
                width = encode_width,
                height = encode_height,
                src_width = job.width,
                src_height = job.height
            );
            let _encode_frame_guard = encode_frame_span.enter();

            // 前処理: RGBA→YUV変換を span で計測
            let rgba_to_yuv_span = span!(Level::DEBUG, "rgba_to_yuv");
            let _rgba_to_yuv_guard = rgba_to_yuv_span.enter();
            let rgba_src = &job.rgba;
            let src_width = job.width as usize;
            let dst_width = encode_width as usize;
            let dst_height = encode_height as usize;

            let yuv_data = rgba_to_yuv::rgba_to_yuv420(rgba_src, dst_width, dst_height, src_width);
            let yuv = YUVBuffer::from_vec(yuv_data, dst_width, dst_height);
            drop(_rgba_to_yuv_guard);

            // 最初のフレームでエンコーダーを作成
            if encoder.is_none() {
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

            // キーフレーム要求がある場合は強制
            if job.request_keyframe {
                encoder.force_intra_frame();
            }

            // エンコードを span で計測
            let encode_span = span!(Level::DEBUG, "encode");
            let _encode_guard = encode_span.enter();
            match encoder.encode(&yuv) {
                Ok(bitstream) => {
                    drop(_encode_guard);

                    // パック処理を span で計測
                    let pack_span = span!(Level::DEBUG, "pack");
                    let _pack_guard = pack_span.enter();
                    let (sample_data, has_sps_pps) = annexb::annexb_from_bitstream(&bitstream);
                    drop(_pack_guard);

                    let sample_size = sample_data.len();
                    drop(_encode_frame_guard);

                    if sample_size == 0 {
                        empty_samples += 1;
                        warn!(
                            "encoder worker: empty sample, skipping (total empty: {})",
                            empty_samples
                        );
                        continue;
                    }

                    successful_encodes += 1;

                    if res_tx
                        .send(EncodeResult {
                            sample_data,
                            is_keyframe: has_sps_pps,
                            duration: job.duration,
                            width: encode_width,
                            height: encode_height,
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

        info!(
            "encoder worker: exiting (successful: {}, failures: {}, empty samples: {})",
            successful_encodes, encode_failures, empty_samples
        );
    });

    (job_queue, res_rx)
}

/// エンコードワーカーを起動する
pub fn start_encode_workers() -> (
    Arc<EncodeJobQueue>,
    tokio_mpsc::UnboundedReceiver<EncodeResult>,
) {
    // encoderの整合性を保つため、常に1つのワーカーのみを起動
    // Pフレームが適切に参照フレームを参照できるようにする
    start_encode_worker()
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
        // Bufferbasedモードはフレームスキップが不要で、バッファ状態に基づいて品質を調整する
        .rate_control_mode(RateControlMode::Bufferbased)
        .num_threads(num_threads);
    openh264::encoder::Encoder::with_api_config(OpenH264API::from_source(), encoder_config)
        .context("Failed to create OpenH264 encoder")
}
