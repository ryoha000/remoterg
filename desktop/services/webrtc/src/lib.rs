mod connection;

use anyhow::Result;
use core_types::VideoStreamMessage;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use webrtc_rs::peer_connection::RTCPeerConnection;

use core_types::{DataChannelMessage, SignalingResponse, WebRtcMessage};

use connection::{handle_add_ice_candidate, handle_set_offer};

/// WebRTCサービス
pub struct WebRtcService {
    message_rx: mpsc::Receiver<WebRtcMessage>,
    signaling_tx: mpsc::Sender<SignalingResponse>,
    data_channel_tx: mpsc::Sender<DataChannelMessage>,
    video_track_tx: Option<
        tokio::sync::oneshot::Sender<(
            Arc<webrtc_rs::track::track_local::track_local_static_sample::TrackLocalStaticSample>,
            Arc<webrtc_rs::rtp_transceiver::rtp_sender::RTCRtpSender>,
            Arc<AtomicBool>, // connection_ready
        )>,
    >,
    video_stream_msg_tx: Option<mpsc::Sender<VideoStreamMessage>>,
    audio_track_tx: Option<
        tokio::sync::oneshot::Sender<(
            Arc<webrtc_rs::track::track_local::track_local_static_sample::TrackLocalStaticSample>,
            Arc<webrtc_rs::rtp_transceiver::rtp_sender::RTCRtpSender>,
        )>,
    >,
}

impl WebRtcService {
    pub fn new(
        signaling_tx: mpsc::Sender<SignalingResponse>,
        data_channel_tx: mpsc::Sender<DataChannelMessage>,
        video_track_tx: Option<
            tokio::sync::oneshot::Sender<(
                Arc<webrtc_rs::track::track_local::track_local_static_sample::TrackLocalStaticSample>,
                Arc<webrtc_rs::rtp_transceiver::rtp_sender::RTCRtpSender>,
                Arc<AtomicBool>,
            )>,
        >,
        video_stream_msg_tx: Option<mpsc::Sender<VideoStreamMessage>>,
        audio_track_tx: Option<
            tokio::sync::oneshot::Sender<(
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
                video_track_tx,
                video_stream_msg_tx,
                audio_track_tx,
            },
            message_tx,
        )
    }

    pub async fn run(mut self) -> Result<()> {
        info!("WebRtcService started");

        // ICE/DTLS が接続完了したかを共有するフラグ（接続前は送出しない）
        let connection_ready = Arc::new(AtomicBool::new(false));

        let mut peer_connection: Option<Arc<RTCPeerConnection>> = None;

        loop {
            tokio::select! {
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
                            ).await {
                                Ok(result) => {
                                    peer_connection = Some(result.peer_connection.clone());

                                    // ビデオトラック情報をVideoStreamServiceに送信
                                    if let Some(tx) = self.video_track_tx.take() {
                                        if tx.send((result.video_track, result.video_sender, connection_ready.clone())).is_ok() {
                                            info!("Video track sent to VideoStreamService");
                                        } else {
                                            warn!("Failed to send video track: receiver dropped");
                                        }
                                    }

                                    // 音声トラックをAudioStreamServiceに送信
                                    if let Some(tx) = self.audio_track_tx.take() {
                                        if tx.send((result.audio_track, result.audio_sender)).is_ok() {
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
