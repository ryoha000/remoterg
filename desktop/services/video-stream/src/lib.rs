mod frame_processor;
mod track_writer;

use anyhow::Result;
use core_types::{Frame, VideoEncoderFactory, VideoStreamMessage};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tracing::{debug, info};
use webrtc_rs::rtp_transceiver::rtp_sender::RTCRtpSender;
use webrtc_rs::track::track_local::track_local_static_sample::TrackLocalStaticSample;

/// VideoStreamService
/// 責務: ビデオフレーム受信 → エンコード → ビデオトラック書き込み
pub struct VideoStreamService {
    frame_rx: mpsc::Receiver<Frame>,
    video_encoder_factory: Arc<dyn VideoEncoderFactory>,
    video_stream_msg_rx: mpsc::Receiver<VideoStreamMessage>,
}

impl VideoStreamService {
    /// 新しいVideoStreamServiceを作成
    pub fn new(
        frame_rx: mpsc::Receiver<Frame>,
        video_encoder_factory: Arc<dyn VideoEncoderFactory>,
        video_stream_msg_rx: mpsc::Receiver<VideoStreamMessage>,
    ) -> Self {
        info!("VideoStreamService::new");
        Self {
            frame_rx,
            video_encoder_factory,
            video_stream_msg_rx,
        }
    }

    /// サービスを実行（ブロッキング）
    /// ビデオトラックとRTPSenderを受け取り、エンコード結果を書き込む
    pub async fn run(
        mut self,
        video_track: Arc<TrackLocalStaticSample>,
        video_sender: Arc<RTCRtpSender>,
        connection_ready: Arc<AtomicBool>,
    ) -> Result<()> {
        info!("VideoStreamService started");

        // エンコーダーをセットアップ
        let (encode_job_slot, mut encode_result_rx) = self.video_encoder_factory.setup();

        // キーフレーム要求フラグ
        let keyframe_requested = Arc::new(AtomicBool::new(false));

        // ビデオフレームをエンコーダーに転送するタスクをスポーン
        let keyframe_requested_clone = keyframe_requested.clone();
        let connection_ready_clone = connection_ready.clone();
        let frame_router_handle = tokio::spawn(async move {
            frame_processor::run_frame_router(
                self.frame_rx,
                encode_job_slot,
                self.video_encoder_factory.clone(),
                connection_ready_clone,
                keyframe_requested_clone,
            )
            .await
        });

        // Video用のRTCPドレインループ
        let video_sender_for_rtcp = video_sender.clone();
        tokio::spawn(async move {
            let mut rtcp_buf = vec![0u8; 1500];
            while let Ok((_, _)) = video_sender_for_rtcp.read(&mut rtcp_buf).await {}
        });

        // 統計情報
        let mut video_frame_count: u64 = 0;
        let mut last_video_log = Instant::now();

        // エンコード結果受信ループ
        loop {
            tokio::select! {
                // エンコード結果受信
                result = encode_result_rx.recv() => {
                    match result {
                        Some(encode_result) => {
                            debug!(
                                "Received video encode result: {} bytes, keyframe: {}",
                                encode_result.sample_data.len(),
                                encode_result.is_keyframe
                            );

                            // トラック書き込み
                            track_writer::write_encoded_sample(
                                &video_track,
                                encode_result,
                            ).await?;

                            video_frame_count += 1;

                            // 統計ログ（5秒ごと）
                            let elapsed = last_video_log.elapsed();
                            if elapsed.as_secs_f32() >= 5.0 {
                                info!(
                                    "Video frames sent: {} (last {}s, {:.1} fps)",
                                    video_frame_count,
                                    elapsed.as_secs(),
                                    video_frame_count as f32 / elapsed.as_secs_f32()
                                );
                                video_frame_count = 0;
                                last_video_log = Instant::now();
                            }
                        }
                        None => {
                            info!("Video encode result channel closed");
                            break;
                        }
                    }
                }
                // キーフレーム要求メッセージ受信
                msg = self.video_stream_msg_rx.recv() => {
                    match msg {
                        Some(VideoStreamMessage::RequestKeyframe) => {
                            debug!("Received keyframe request");
                            keyframe_requested.store(true, Ordering::Relaxed);
                        }
                        None => {
                            info!("Video stream message channel closed");
                            break;
                        }
                    }
                }
            }
        }

        // フレームルーターの終了を待つ
        let _ = frame_router_handle.await;

        info!("VideoStreamService stopped");
        Ok(())
    }
}
