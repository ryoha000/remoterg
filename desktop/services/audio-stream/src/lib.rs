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
        mut track_rx: mpsc::Receiver<(
            Arc<TrackLocalStaticSample>,
            Arc<RTCRtpSender>,
        )>,
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

        // 統計情報
        let mut audio_frame_count: u64 = 0;
        let mut audio_silent_count: u64 = 0;
        let mut last_audio_log = Instant::now();

        // 現在のアクティブなトラック情報
        let mut current_audio_track: Option<Arc<TrackLocalStaticSample>> = None;

        // RTCP読み込みタスクのハンドル（キャンセル用）
        let mut rtcp_drain_handle: Option<tokio::task::JoinHandle<()>> = None;

        info!("AudioStreamService entered main loop");

        // メインループ
        loop {
            tokio::select! {
                // 1. 新しいトラック情報の受信
                new_track = track_rx.recv() => {
                    match new_track {
                        Some((track, sender)) => {
                            info!("Switched to new audio track");

                            // 古いRTCPタスクをキャンセル
                            if let Some(handle) = rtcp_drain_handle.take() {
                                handle.abort();
                            }

                            // 新しいRTCPタスクを起動
                            let sender_for_rtcp = sender.clone();
                            rtcp_drain_handle = Some(tokio::spawn(async move {
                                let mut rtcp_buf = vec![0u8; 1500];
                                while let Ok((_, _)) = sender_for_rtcp.read(&mut rtcp_buf).await {}
                            }));

                             // 明示的な送信開始
                            let sender_for_start = sender.clone();
                            tokio::spawn(async move {
                                let params = sender_for_start.get_parameters().await;
                                if let Err(e) = sender_for_start.send(&params).await {
                                    warn!("Audio RTCRtpSender::send() explicit call returned: {}", e);
                                }
                            });

                            // ステート更新
                            current_audio_track = Some(track);
                        }
                        None => {
                            info!("Audio track channel closed");
                            break;
                        }
                    }
                }

                // 2. エンコード結果の受信と送信
                result = audio_result_rx.recv() => {
                    match result {
                        Some(result) => {
                             if let Some(track) = &current_audio_track {
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

                                match track.write_sample(&sample).await {
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
                        }
                        None => {
                            info!("Audio encode result channel closed");
                            break;
                        }
                    }
                }
            }
        }

        // クリーンアップ
        if let Some(handle) = rtcp_drain_handle {
            handle.abort();
        }
        let _ = frame_router_handle.await;

        info!("AudioStreamService stopped");
        Ok(())
    }
}
