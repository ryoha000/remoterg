mod connection;
mod frame_handler;
mod track_writer;

use anyhow::{bail, Result};
use core_types::{EncodeResult, VideoCodec, VideoEncoderFactory};
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use webrtc_rs::peer_connection::RTCPeerConnection;

use core_types::{DataChannelMessage, Frame, SignalingResponse, WebRtcMessage};

use connection::{handle_add_ice_candidate, handle_set_offer};
use frame_handler::{log_performance_stats, process_frame, FrameStats};
use track_writer::{handle_keyframe_request, process_encode_result, VideoTrackState};

/// WebRTCサービス
pub struct WebRtcService {
    frame_rx: mpsc::Receiver<Frame>,
    message_rx: mpsc::Receiver<WebRtcMessage>,
    signaling_tx: mpsc::Sender<SignalingResponse>,
    data_channel_tx: mpsc::Sender<DataChannelMessage>,
    encoder_factories: HashMap<VideoCodec, Arc<dyn VideoEncoderFactory>>,
}

impl WebRtcService {
    pub fn new(
        frame_rx: mpsc::Receiver<Frame>,
        signaling_tx: mpsc::Sender<SignalingResponse>,
        data_channel_tx: mpsc::Sender<DataChannelMessage>,
        encoder_factories: HashMap<VideoCodec, Arc<dyn VideoEncoderFactory>>,
    ) -> (Self, mpsc::Sender<WebRtcMessage>) {
        let (message_tx, message_rx) = mpsc::channel(100);
        (
            Self {
                frame_rx,
                message_rx,
                signaling_tx,
                data_channel_tx,
                encoder_factories,
            },
            message_tx,
        )
    }

    fn select_encoder_factory(
        &self,
        requested: Option<VideoCodec>,
    ) -> Result<(Arc<dyn VideoEncoderFactory>, VideoCodec)> {
        if let Some(codec) = requested {
            if let Some(factory) = self.encoder_factories.get(&codec) {
                return Ok((factory.clone(), codec));
            } else {
                bail!(
                    "要求されたコーデックはビルドで有効化されていません: {:?}",
                    codec
                );
            }
        }

        for codec in [VideoCodec::H264] {
            if let Some(factory) = self.encoder_factories.get(&codec) {
                return Ok((factory.clone(), codec));
            }
        }

        bail!("利用可能なエンコーダが存在しません（feature未有効）");
    }

    pub async fn run(mut self) -> Result<()> {
        info!("WebRtcService started");

        // PLI/FIR などの RTCP フィードバックに応じてキーフレーム再送を行うための通知チャネル
        let (keyframe_tx, mut keyframe_rx) = mpsc::unbounded_channel::<()>();
        // ICE/DTLS が接続完了したかを共有するフラグ（接続前は送出しない）
        let connection_ready = Arc::new(AtomicBool::new(false));

        let mut peer_connection: Option<Arc<RTCPeerConnection>> = None;
        let mut video_track_state: Option<VideoTrackState> = None;
        let mut encode_job_slot: Option<Arc<core_types::EncodeJobSlot>> = None;
        let mut encode_result_rx: Option<tokio::sync::mpsc::UnboundedReceiver<EncodeResult>> = None;

        let mut frame_count: u64 = 0;
        let mut last_frame_ts: Option<u64> = None;
        let mut last_frame_log = Instant::now();
        let mut frame_stats = FrameStats::new();
        // キーフレーム要求フラグ（PLI/FIR受信時に設定）
        let keyframe_requested = Arc::new(AtomicBool::new(false));

        loop {
            tokio::select! {
                // フレーム受信
                frame = self.frame_rx.recv() => {
                    match frame {
                        Some(frame) => {
                            let pipeline_start = Instant::now();

                            // 解像度変更を検出した場合はencoderを再生成
                            let resolution_changed = process_frame(
                                frame,
                                video_track_state.as_mut(),
                                encode_job_slot.as_ref(),
                                &connection_ready,
                                &keyframe_requested,
                                &mut last_frame_ts,
                                &mut frame_stats,
                                pipeline_start,
                            );

                            if resolution_changed.is_some() {
                                // 既存のencoderワーカーを停止（シャットダウンしてからドロップ）
                                if let Some(old_slot) = encode_job_slot.as_ref() {
                                    old_slot.shutdown();
                                }
                                drop(encode_job_slot.take());
                                drop(encode_result_rx.take());

                                // 新しいencoderワーカーを起動
                                if let Some(ref track_state) = video_track_state {
                                    let (job_slot, res_rx) = track_state.encoder_factory.setup();
                                    encode_job_slot = Some(job_slot);
                                    encode_result_rx = Some(res_rx);
                                }
                            }

                            // パフォーマンス統計を定期的に出力
                            log_performance_stats(&mut frame_stats);
                        }
                        None => {
                            debug!("Frame channel closed");
                            break;
                        }
                    }
                }
                // エンコード結果受信（ワーカー -> メイン）
                encoded = async {
                    if let Some(rx) = encode_result_rx.as_mut() {
                        rx.recv().await
                    } else {
                        None
                    }
                } => {
                    if let Some(result) = encoded {
                        if let Some(ref mut track_state) = video_track_state {
                            process_encode_result(
                                result,
                                track_state,
                                &mut frame_count,
                                &mut last_frame_log,
                            ).await;
                        } else {
                            debug!("Received encoded frame but video track is not ready");
                        }
                    }
                }
                // PLI/FIR によるキーフレーム再送要求
                keyframe_req = keyframe_rx.recv() => {
                    if keyframe_req.is_some() {
                        if let Some(ref mut track_state) = video_track_state {
                            handle_keyframe_request(track_state, &keyframe_requested);
                        }
                    }
                }
                // メッセージ受信
                msg = self.message_rx.recv() => {
                    match msg {
                        Some(WebRtcMessage::SetOffer { sdp, codec }) => {
                            let (encoder_factory, selected_codec) =
                                match self.select_encoder_factory(codec) {
                                    Ok(res) => res,
                                    Err(e) => {
                                        warn!("{}", e);
                                        let _ = self
                                            .signaling_tx
                                            .send(SignalingResponse::Error {
                                                message: e.to_string(),
                                            })
                                            .await;
                                        continue;
                                    }
                                };

                            match handle_set_offer(
                                sdp,
                                codec,
                                encoder_factory.clone(),
                                selected_codec,
                                self.signaling_tx.clone(),
                                self.data_channel_tx.clone(),
                                connection_ready.clone(),
                                keyframe_tx.clone(),
                            ).await {
                                Ok(result) => {
                                    peer_connection = Some(result.peer_connection);
                                    video_track_state = Some(result.video_track_state);
                                    encode_job_slot = Some(result.encode_job_slot);
                                    encode_result_rx = Some(result.encode_result_rx);
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
                        Some(WebRtcMessage::AddIceCandidate { candidate, sdp_mid, sdp_mline_index }) => {
                            if let Some(ref pc) = peer_connection {
                                if let Err(e) = handle_add_ice_candidate(
                                    pc,
                                    candidate,
                                    sdp_mid,
                                    sdp_mline_index,
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
