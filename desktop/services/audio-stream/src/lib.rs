use anyhow::Result;
use core_types::{AudioEncoderFactory, AudioFrame};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};
use webrtc_rs::rtp_transceiver::rtp_sender::RTCRtpSender;
use webrtc_rs::track::track_local::track_local_static_sample::TrackLocalStaticSample;

/// AudioStreamService
/// 責務: 音声フレーム受信 → エンコード → 音声トラック書き込み
pub struct AudioStreamService {
    audio_frame_rx: mpsc::Receiver<AudioFrame>,
    audio_encoder_factory: Arc<dyn AudioEncoderFactory>,
}

impl AudioStreamService {
    /// 新しいAudioStreamServiceを作成
    pub fn new(
        audio_frame_rx: mpsc::Receiver<AudioFrame>,
        audio_encoder_factory: Arc<dyn AudioEncoderFactory>,
    ) -> Self {
        info!("AudioStreamService::new");
        Self {
            audio_frame_rx,
            audio_encoder_factory,
        }
    }

    /// サービスを実行（ブロッキング）
    /// 音声トラックとRTPSenderを受け取り、エンコード結果を書き込む
    pub async fn run(
        mut self,
        audio_track: Arc<TrackLocalStaticSample>,
        audio_sender: Arc<RTCRtpSender>,
    ) -> Result<()> {
        info!("AudioStreamService started");

        // エンコーダーをセットアップ
        let (audio_encoder_tx, mut audio_result_rx) = self.audio_encoder_factory.setup();

        // 音声フレームをエンコーダーに転送するタスクをスポーン
        let frame_router_handle = tokio::spawn(async move {
            while let Some(frame) = self.audio_frame_rx.recv().await {
                if audio_encoder_tx.send(frame).await.is_err() {
                    debug!("Audio encoder channel closed");
                    break;
                }
            }
        });

        // Audio用のRTCPドレインループ
        let audio_sender_for_rtcp = audio_sender.clone();
        tokio::spawn(async move {
            let mut rtcp_buf = vec![0u8; 1500];
            while let Ok((_, _)) = audio_sender_for_rtcp.read(&mut rtcp_buf).await {}
        });

        // 統計情報
        let mut audio_frame_count: u64 = 0;
        let mut audio_silent_count: u64 = 0;
        let mut last_audio_log = Instant::now();

        // エンコード結果受信ループ
        loop {
            match audio_result_rx.recv().await {
                Some(result) => {
                    debug!(
                        "Received audio encode result: {} bytes, silent: {}",
                        result.encoded_data.len(),
                        result.is_silent
                    );

                    use bytes::Bytes;
                    use webrtc_rs::media::Sample;
                    let sample = Sample {
                        data: Bytes::from(result.encoded_data),
                        duration: result.duration,
                        ..Default::default()
                    };

                    match audio_track.write_sample(&sample).await {
                        Ok(_) => {
                            audio_frame_count += 1;
                            if result.is_silent {
                                audio_silent_count += 1;
                            }
                            let elapsed = last_audio_log.elapsed();
                            if elapsed.as_secs_f32() >= 5.0 {
                                if audio_silent_count == audio_frame_count && audio_frame_count > 0
                                {
                                    warn!(
                                        "Audio frames sent: {} (last {}s) - ALL FRAMES ARE SILENT! No audio detected.",
                                        audio_frame_count,
                                        elapsed.as_secs()
                                    );
                                } else {
                                    info!(
                                        "Audio frames sent: {} (last {}s), silent: {} ({:.1}%)",
                                        audio_frame_count,
                                        elapsed.as_secs(),
                                        audio_silent_count,
                                        (audio_silent_count as f32 / audio_frame_count as f32)
                                            * 100.0
                                    );
                                }
                                audio_frame_count = 0;
                                audio_silent_count = 0;
                                last_audio_log = Instant::now();
                            }
                        }
                        Err(e) => {
                            error!("Failed to write audio sample to track: {}", e);
                        }
                    }
                }
                None => {
                    info!("Audio encode result channel closed");
                    break;
                }
            }
        }

        // フレームルーターの終了を待つ
        let _ = frame_router_handle.await;

        info!("AudioStreamService stopped");
        Ok(())
    }
}
