mod connection;

use anyhow::Result;
use core_types::VideoStreamMessage;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use webrtc_rs::peer_connection::RTCPeerConnection;

use std::sync::Mutex;
use core_types::{DataChannelMessage, OutgoingDataChannelMessage, SignalingResponse, WebRtcMessage};

use connection::{handle_add_ice_candidate, handle_set_offer};

/// WebRTCサービス
pub struct WebRtcService {
    message_rx: mpsc::Receiver<WebRtcMessage>,
    signaling_tx: mpsc::Sender<SignalingResponse>,
    data_channel_tx: mpsc::Sender<DataChannelMessage>,
    outgoing_data_channel_rx: Option<mpsc::Receiver<OutgoingDataChannelMessage>>,
    video_track_tx: Option<
        mpsc::Sender<(
            Arc<webrtc_rs::track::track_local::track_local_static_sample::TrackLocalStaticSample>,
            Arc<webrtc_rs::rtp_transceiver::rtp_sender::RTCRtpSender>,
            Arc<AtomicBool>, // connection_ready
        )>,
    >,
    video_stream_msg_tx: Option<mpsc::Sender<VideoStreamMessage>>,
    audio_track_tx: Option<
        mpsc::Sender<(
            Arc<webrtc_rs::track::track_local::track_local_static_sample::TrackLocalStaticSample>,
            Arc<webrtc_rs::rtp_transceiver::rtp_sender::RTCRtpSender>,
        )>,
    >,
}

impl WebRtcService {
    pub fn new(
        signaling_tx: mpsc::Sender<SignalingResponse>,
        data_channel_tx: mpsc::Sender<DataChannelMessage>,
        outgoing_data_channel_rx: Option<mpsc::Receiver<OutgoingDataChannelMessage>>,
        video_track_tx: Option<
            mpsc::Sender<(
                Arc<webrtc_rs::track::track_local::track_local_static_sample::TrackLocalStaticSample>,
                Arc<webrtc_rs::rtp_transceiver::rtp_sender::RTCRtpSender>,
                Arc<AtomicBool>,
            )>,
        >,
        video_stream_msg_tx: Option<mpsc::Sender<VideoStreamMessage>>,
        audio_track_tx: Option<
            mpsc::Sender<(
                Arc<webrtc_rs::track::track_local::track_local_static_sample::TrackLocalStaticSample>,
                Arc<webrtc_rs::rtp_transceiver::rtp_sender::RTCRtpSender>,
            )>,
        >,
    ) -> (Self, mpsc::Sender<WebRtcMessage>) {
        let (message_tx, message_rx) = mpsc::channel(100);
        (
            Self {
                message_rx,
                signaling_tx,
                data_channel_tx,
                outgoing_data_channel_rx,
                video_track_tx,
                video_stream_msg_tx,
                audio_track_tx,
            },
            message_tx,
        )
    }

    /// ICE Restartを実行
    async fn execute_ice_restart(
        &self,
        peer_connection: &Arc<RTCPeerConnection>,
    ) -> Result<()> {
        use anyhow::Context;

        info!("Executing ICE Restart...");

        // 1. restart_ice()を呼び出し（新しいICE credentialsを生成）
        peer_connection.restart_ice().await
            .context("Failed to restart ICE")?;

        // 2. 新しいOfferを生成
        let offer = peer_connection
            .create_offer(None)
            .await
            .context("Failed to create offer for ICE restart")?;

        info!("ICE Restart offer generated:\n{}", offer.sdp);

        // 3. LocalDescriptionとして設定
        peer_connection
            .set_local_description(offer.clone())
            .await
            .context("Failed to set local description for ICE restart")?;

        // 4. シグナリングサービスに送信
        self.signaling_tx
            .send(SignalingResponse::OfferForRestart { sdp: offer.sdp })
            .await
            .context("Failed to send offer for ICE restart")?;

        info!("ICE Restart offer sent to signaling service");
        Ok(())
    }

    pub async fn run(mut self, webrtc_msg_tx: mpsc::Sender<WebRtcMessage>) -> Result<()> {
        info!("WebRtcService started");

        // ICE/DTLS が接続完了したかを共有するフラグ（接続前は送出しない）
        let connection_ready = Arc::new(AtomicBool::new(false));

        // アクティブなデータチャネルを保持（outgoing用）
        let active_data_channel = Arc::new(Mutex::new(None::<Arc<webrtc_rs::data_channel::RTCDataChannel>>));

        let mut peer_connection: Option<Arc<RTCPeerConnection>> = None;

        loop {
            tokio::select! {
                // Outgoing DataChannel messages
                msg = async {
                    if let Some(rx) = &mut self.outgoing_data_channel_rx {
                         rx.recv().await
                    } else {
                        // If no receiver, wait forever
                        std::future::pending::<Option<OutgoingDataChannelMessage>>().await
                    }
                } => {
                    match msg {
                        Some(outgoing_msg) => {
                             let dc_opt = active_data_channel.lock().unwrap().clone();
                             if let Some(dc) = dc_opt {
                                 match outgoing_msg {
                                     OutgoingDataChannelMessage::Text(data_msg) => {
                                         if let Ok(json) = serde_json::to_string(&data_msg) {
                                            if let Err(e) = dc.send_text(json).await {
                                                warn!("Failed to send text data channel message: {}", e);
                                            }
                                         }
                                     }
                                     OutgoingDataChannelMessage::Binary(bytes) => {
                                         use bytes::Bytes;
                                         if let Err(e) = dc.send(&Bytes::from(bytes)).await {
                                             warn!("Failed to send binary data channel message: {}", e);
                                         }
                                     }
                                 }
                             } else {
                                 warn!("Cannot send data channel message: no active data channel");
                             }
                        }
                        None => {
                            debug!("Outgoing data channel closed");
                            // Don't break loop, just stop processing outgoing
                            self.outgoing_data_channel_rx = None;
                        }
                    }
                }

                // メッセージ受信
                msg = self.message_rx.recv() => {
                    match msg {
                        Some(WebRtcMessage::SetOffer { sdp, codec }) => {
                            info!("Received SetOffer message (codec: {:?})", codec);
                            // 既存のPeerConnectionが存在する場合はクリーンアップ
                            if peer_connection.is_some() {
                                info!("Cleaning up existing PeerConnection before creating new one");

                                // 既存のPeerConnectionをクリーンアップ
                                if let Some(old_pc) = peer_connection.take() {
                                    if let Err(e) = old_pc.close().await {
                                        warn!("Failed to close existing PeerConnection: {}", e);
                                    } else {
                                        info!("Existing PeerConnection closed");
                                    }
                                }

                                // connection_readyフラグをリセット
                                connection_ready.store(false, std::sync::atomic::Ordering::Relaxed);
                                // active_data_channelもリセット
                                *active_data_channel.lock().unwrap() = None;
                            }

                            // video_stream_msg_tx を取得（None の場合は後続処理をスキップ）
                            let video_stream_msg_tx = match self.video_stream_msg_tx.clone() {
                                Some(tx) => tx,
                                None => {
                                    warn!("video_stream_msg_tx is None, skipping SetOffer");
                                    continue;
                                }
                            };

                            match handle_set_offer(
                                sdp,
                                codec,
                                self.signaling_tx.clone(),
                                self.data_channel_tx.clone(),
                                connection_ready.clone(),
                                video_stream_msg_tx,
                                webrtc_msg_tx.clone(),
                                active_data_channel.clone(),
                            ).await {
                                Ok(result) => {
                                    peer_connection = Some(result.peer_connection.clone());

                                    // ビデオトラック情報をVideoStreamServiceに送信
                                    if let Some(ref tx) = self.video_track_tx {
                                        if tx.send((result.video_track, result.video_sender, connection_ready.clone())).await.is_ok() {
                                            info!("Video track sent to VideoStreamService");
                                        } else {
                                            warn!("Failed to send video track: receiver dropped");
                                        }
                                    }

                                    // 音声トラックをAudioStreamServiceに送信
                                    if let Some(ref tx) = self.audio_track_tx {
                                        if tx.send((result.audio_track, result.audio_sender)).await.is_ok() {
                                            info!("Audio track sent to AudioStreamService");
                                        } else {
                                            warn!("Failed to send audio track: receiver dropped");
                                        }
                                    }
                                }
                                Err(e) => {
                                    warn!("Failed to handle SetOffer: {}", e);
                                    let _ = self
                                        .signaling_tx
                                        .send(SignalingResponse::Error {
                                            message: e.to_string(),
                                        })
                                        .await;
                                }
                            }
                        }
                        Some(WebRtcMessage::AddIceCandidate { candidate, sdp_mid, sdp_mline_index, username_fragment }) => {
                            if let Some(ref pc) = peer_connection {
                                if let Err(e) = handle_add_ice_candidate(
                                    pc,
                                    candidate,
                                    sdp_mid,
                                    sdp_mline_index,
                                    username_fragment,
                                ).await {
                                    warn!("Failed to add ICE candidate: {}", e);
                                }
                            } else {
                                warn!("Received ICE candidate but no peer connection exists");
                            }
                        }
                        Some(WebRtcMessage::TriggerIceRestart) => {
                            if let Some(ref pc) = peer_connection {
                                info!("Received TriggerIceRestart message");
                                if let Err(e) = self.execute_ice_restart(pc).await {
                                    warn!("Failed to execute ICE restart: {}", e);
                                    let _ = self
                                        .signaling_tx
                                        .send(SignalingResponse::Error {
                                            message: format!("ICE Restart failed: {}", e),
                                        })
                                        .await;
                                }
                            } else {
                                warn!("Cannot restart ICE: no peer connection exists");
                            }
                        }
                        Some(WebRtcMessage::SetAnswerForRestart { sdp }) => {
                            if let Some(ref pc) = peer_connection {
                                info!("Received Answer for ICE restart");
                                match webrtc_rs::peer_connection::sdp::session_description::RTCSessionDescription::answer(sdp) {
                                    Ok(answer) => {
                                        match pc.set_remote_description(answer).await {
                                            Ok(_) => {
                                                info!("ICE Restart completed successfully");
                                                // connection_readyフラグは、ICE状態変更ハンドラで自動的にtrueに設定される
                                            }
                                            Err(e) => {
                                                warn!("Failed to set remote description for ICE restart: {}", e);
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        warn!("Failed to parse answer SDP for ICE restart: {}", e);
                                    }
                                }
                            } else {
                                warn!("Cannot set answer for ICE restart: no peer connection exists");
                            }
                        }
                        None => {
                            debug!("Message channel closed");
                            break;
                        }
                    }
                }
            }
        }

        // PeerConnectionをクリーンアップ
        if let Some(pc) = peer_connection {
            let _ = pc.close().await;
        }

        info!("WebRtcService stopped");
        Ok(())
    }
}
