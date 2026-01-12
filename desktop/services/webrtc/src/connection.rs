use anyhow::{Context, Result};
use core_types::{DataChannelMessage, SignalingResponse, VideoCodec, VideoStreamMessage};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};
use webrtc_rs::api::interceptor_registry::register_default_interceptors;
use webrtc_rs::api::media_engine::{MediaEngine, MIME_TYPE_H264, MIME_TYPE_OPUS};
use webrtc_rs::api::setting_engine::SettingEngine;
use webrtc_rs::api::APIBuilder;
use webrtc_rs::data_channel::data_channel_message::DataChannelMessage as RTCDataChannelMessage;
use webrtc_rs::data_channel::RTCDataChannel;
use webrtc_rs::ice_transport::ice_candidate::RTCIceCandidate;
use webrtc_rs::ice_transport::ice_candidate::RTCIceCandidateInit;
use webrtc_rs::ice_transport::ice_server::RTCIceServer;
use webrtc_rs::interceptor::registry::Registry;
use webrtc_rs::peer_connection::configuration::RTCConfiguration;
use webrtc_rs::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc_rs::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc_rs::peer_connection::RTCPeerConnection;
use webrtc_rs::rtcp::payload_feedbacks::full_intra_request::FullIntraRequest;
use webrtc_rs::rtcp::payload_feedbacks::picture_loss_indication::PictureLossIndication;
use webrtc_rs::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;
use webrtc_rs::rtp_transceiver::rtp_sender::RTCRtpSender;
use webrtc_rs::track::track_local::track_local_static_sample::TrackLocalStaticSample;
use webrtc_rs::track::track_local::TrackLocal;

/// RTCIceCandidateから完全なSDP candidate文字列を生成
///
/// 注意: この関数はICE candidate送信では使用しない。
/// ICE candidate送信にはRTCIceCandidate::to_json()を使用する。
#[allow(dead_code)]
pub fn format_ice_candidate(candidate: &RTCIceCandidate) -> String {
    // webrtc-rsのRTCIceCandidateから完全なSDP candidate文字列を生成
    // フォーマット: candidate:<foundation> <component> <protocol> <priority> <address> <port> typ <type> [raddr <raddr>] [rport <rport>] [generation <generation>]

    let mut candidate_str = format!(
        "candidate:{} {} {} {} {} {}",
        candidate.foundation,
        candidate.component,
        candidate.protocol,
        candidate.priority,
        candidate.address,
        candidate.port
    );

    // candidate typeを追加
    // RTCIceCandidateにはcandidate_typeフィールドがある可能性があるが、
    // 実際の構造を確認する必要がある。とりあえず、addressから推測する
    let candidate_type = if candidate.address.starts_with("127.")
        || candidate.address.starts_with("192.168.")
        || candidate.address.starts_with("10.")
        || candidate.address.starts_with("172.")
        || candidate.address == "::1"
        || candidate.address.starts_with("fe80:")
    {
        "host"
    } else if candidate.address.starts_with("169.254.") {
        "host" // Link-local address
    } else {
        "srflx" // Server reflexive (STUN経由)
    };

    candidate_str.push_str(&format!(" typ {}", candidate_type));

    // related addressがある場合は追加
    // RTCIceCandidateにはrelated_addressフィールドがある可能性があるが、
    // 実際の構造を確認する必要がある

    candidate_str
}

pub fn codec_to_mime_type(codec: VideoCodec) -> String {
    match codec {
        VideoCodec::H264 => MIME_TYPE_H264.to_owned(),
    }
}

/// SetOfferメッセージの処理結果
pub struct SetOfferResult {
    pub peer_connection: Arc<RTCPeerConnection>,
    pub video_track: Arc<TrackLocalStaticSample>,
    pub video_sender: Arc<RTCRtpSender>,
    pub audio_track: Option<Arc<TrackLocalStaticSample>>,
    pub audio_sender: Option<Arc<RTCRtpSender>>,
}

/// SetOfferメッセージを処理
pub async fn handle_set_offer(
    sdp: String,
    codec: Option<VideoCodec>,
    signaling_tx: mpsc::Sender<SignalingResponse>,
    data_channel_tx: mpsc::Sender<DataChannelMessage>,
    connection_ready: Arc<AtomicBool>,
    video_stream_msg_tx: mpsc::Sender<VideoStreamMessage>,
) -> Result<SetOfferResult> {
    info!("SetOffer received, generating answer");

    // video codec を選択（デフォルトは H264）
    let selected_codec = codec.unwrap_or(VideoCodec::H264);
    info!("Using video codec: {:?}", selected_codec);

    // webrtc-rsのAPIを初期化
    let mut m = MediaEngine::default();
    m.register_default_codecs()?;

    let mut registry = Registry::new();
    registry = register_default_interceptors(registry, &mut m)?;

    // ループバック候補を含める（同一ホスト内接続を確実にするため）
    let mut setting_engine = SettingEngine::default();
    setting_engine.set_include_loopback_candidate(true);

    // ICE timeout設定: デフォルト5秒では短すぎるため延長
    // - disconnected_timeout: 5秒 → 20秒（ネットワーク活動なしでDisconnected判定される時間）
    // - failed_timeout: 25秒 → 40秒（Disconnected後にFailed判定される時間）
    // - keepalive_interval: 2秒（メディアがない場合に定期的なkeepaliveトラフィック送信）
    // Keepalive実装により3秒ごとにトラフィックが発生するため、安全マージンを確保
    setting_engine.set_ice_timeouts(
        Some(Duration::from_secs(20)), // disconnected_timeout: 15秒 → 20秒
        Some(Duration::from_secs(40)), // failed_timeout: 30秒 → 40秒
        Some(Duration::from_secs(2)),  // keepalive_interval: 変更なし
    );

    let api = APIBuilder::new()
        .with_media_engine(m)
        .with_setting_engine(setting_engine)
        .with_interceptor_registry(registry)
        .build();

    // ICE設定（GoogleのSTUNサーバーを使用）
    let config = RTCConfiguration {
        ice_servers: vec![RTCIceServer {
            urls: vec!["stun:stun.l.google.com:19302".to_string()],
            ..Default::default()
        }],
        ..Default::default()
    };

    // PeerConnectionを作成
    let pc = Arc::new(
        api.new_peer_connection(config)
            .await
            .context("Failed to create peer connection")?,
    );

    // OfferをRemoteDescriptionとして設定
    let offer = RTCSessionDescription::offer(sdp).context("Failed to parse offer SDP")?;
    info!("Offer SDP received:\n{}", offer.sdp);
    pc.set_remote_description(offer)
        .await
        .context("Failed to set remote description")?;

    // Video trackを作成して追加
    let mime_type = codec_to_mime_type(selected_codec);

    let video_track = Arc::new(TrackLocalStaticSample::new(
        RTCRtpCodecCapability {
            mime_type: mime_type.clone(),
            ..Default::default()
        },
        "video".to_string(),
        "stream".to_string(),
    ));

    // Transceiverを追加（sendonly）
    let sender: Arc<RTCRtpSender> = pc
        .add_track(video_track.clone() as Arc<dyn TrackLocal + Send + Sync>)
        .await
        .context("Failed to add video track")?;

    info!("Video track added to peer connection");

    // 音声トラックを追加（オプション）
    let mut audio_track: Option<Arc<TrackLocalStaticSample>> = None;
    let mut audio_sender: Option<Arc<RTCRtpSender>> = None;

    // 音声トラックを常に作成（AudioStreamServiceで使用）
    info!("Adding audio track with Opus codec");
    let audio_track_local = Arc::new(TrackLocalStaticSample::new(
        RTCRtpCodecCapability {
            mime_type: MIME_TYPE_OPUS.to_string(),
            ..Default::default()
        },
        "audio".to_string(),
        "stream".to_string(),
    ));

    let audio_sender_local: Arc<RTCRtpSender> = pc
        .add_track(audio_track_local.clone() as Arc<dyn TrackLocal + Send + Sync>)
        .await
        .context("Failed to add audio track")?;

    info!("Audio track added to peer connection");

    // 音声トラックとsenderを保持（AudioStreamServiceに渡すため）
    audio_track = Some(audio_track_local);
    audio_sender = Some(audio_sender_local);

    // RTCP 受信ループを開始し、PLI/FIR を受けたら VideoStreamService にキーフレーム要求を送信
    let video_stream_msg_tx_rtcp = video_stream_msg_tx.clone();
    let sender_for_rtcp = sender.clone();
    tokio::spawn(async move {
        loop {
            match sender_for_rtcp.read_rtcp().await {
                Ok((pkts, _)) => {
                    for pkt in pkts {
                        if pkt
                            .as_any()
                            .downcast_ref::<PictureLossIndication>()
                            .is_some()
                            || pkt.as_any().downcast_ref::<FullIntraRequest>().is_some()
                        {
                            debug!("RTCP feedback (PLI/FIR) received, requesting keyframe");
                            let _ = video_stream_msg_tx_rtcp
                                .send(VideoStreamMessage::RequestKeyframe)
                                .await;
                        }
                    }
                }
                Err(err) => {
                    debug!("RTCP read loop finished: {}", err);
                    break;
                }
            }
        }
    });

    // DataChannelハンドラを設定
    let dc_tx = data_channel_tx.clone();
    pc.on_data_channel(Box::new(move |dc: Arc<RTCDataChannel>| {
        let dc_tx = dc_tx.clone();
        Box::pin(async move {
            let label = dc.label();
            let label_str = label.to_string();
            info!("DataChannel opened: {}", label_str);

            let dc_tx_on_msg = dc_tx.clone();
            let dc_for_pong = dc.clone();
            dc.on_message(Box::new(move |msg: RTCDataChannelMessage| {
                let dc_tx_on_msg = dc_tx_on_msg.clone();
                let dc_for_pong = dc_for_pong.clone();
                Box::pin(async move {
                    if msg.is_string {
                        if let Ok(text) = String::from_utf8(msg.data.to_vec()) {
                            match serde_json::from_str::<DataChannelMessage>(&text) {
                                Ok(parsed) => {
                                    match &parsed {
                                        DataChannelMessage::Ping { timestamp } => {
                                            debug!("Received keepalive ping from client (timestamp: {})", timestamp);
                                            // Pingを受信したらPongを返信
                                            let pong_msg = DataChannelMessage::Pong { timestamp: *timestamp };
                                            if let Ok(pong_json) = serde_json::to_string(&pong_msg) {
                                                if let Err(e) = dc_for_pong.send_text(pong_json).await {
                                                    warn!("Failed to send pong: {}", e);
                                                } else {
                                                    debug!("Sent pong response (timestamp: {})", timestamp);
                                                }
                                            }
                                        }
                                        DataChannelMessage::Pong { timestamp } => {
                                            debug!("Received keepalive pong from client (timestamp: {})", timestamp);
                                            // Pongメッセージは処理不要（受信だけで十分）
                                        }
                                        _ => {
                                            // その他のメッセージは従来通りinputサービスに転送
                                            if let Err(e) = dc_tx_on_msg.send(parsed).await {
                                                warn!("Failed to forward data channel message: {}", e);
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    warn!("Failed to parse data channel message: {}", e);
                                }
                            }
                        } else {
                            warn!("Received non-UTF8 data channel message");
                        }
                    } else {
                        debug!("Ignoring binary data channel message");
                    }
                })
            }));

            // サーバー側から定期的にPingを送信するタスク
            let dc_for_ping = dc.clone();
            let ping_task_closed = Arc::new(AtomicBool::new(false));
            let ping_task_closed_clone = ping_task_closed.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(3));
                loop {
                    interval.tick().await;
                    // DataChannelが閉じられたかチェック
                    if ping_task_closed_clone.load(Ordering::Relaxed) {
                        debug!("DataChannel closed, stopping ping task");
                        break;
                    }
                    let timestamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as u64;
                    let ping_msg = DataChannelMessage::Ping { timestamp };
                    if let Ok(ping_json) = serde_json::to_string(&ping_msg) {
                        match dc_for_ping.send_text(ping_json).await {
                            Ok(_) => {
                                debug!("Sent keepalive ping to client (timestamp: {})", timestamp);
                            }
                            Err(e) => {
                                warn!("Failed to send ping: {}", e);
                                break; // 送信失敗時はループを終了
                            }
                        }
                    }
                }
            });
            let ping_task_closed_for_close = ping_task_closed.clone();
            dc.on_close(Box::new(move || {
                let label_str = label_str.clone();
                let ping_task_closed_for_close = ping_task_closed_for_close.clone();
                Box::pin(async move {
                    info!("DataChannel closed: {}", label_str);
                    ping_task_closed_for_close.store(true, Ordering::Relaxed);
                })
            }));
        })
    }));

    // Answerを生成
    let answer = pc
        .create_answer(None)
        .await
        .context("Failed to create answer")?;
    info!("Answer SDP generated:\n{}", answer.sdp);

    // ICE candidateのイベントハンドラを LocalDescription 設定前に登録して、
    // 初期ホスト候補を取りこぼさないようにする
    let signaling_tx_ice = signaling_tx.clone();
    pc.on_ice_candidate(Box::new(move |candidate: Option<RTCIceCandidate>| {
        let signaling_tx = signaling_tx_ice.clone();
        Box::pin(async move {
            match candidate {
                Some(candidate) => {
                    // RTCIceCandidate::to_json()を使用してRTCIceCandidateInitを取得
                    // これにより、candidate文字列、sdp_mid、sdp_mline_index、username_fragmentを
                    // 仕様準拠の形式で取得できる
                    match candidate.to_json() {
                        Ok(candidate_init) => {
                            info!(
                                "ICE candidate: {} (mid: {:?}, mline_index: {:?}, username_fragment: {:?})",
                                candidate_init.candidate,
                                candidate_init.sdp_mid,
                                candidate_init.sdp_mline_index,
                                candidate_init.username_fragment
                            );

                            if let Err(e) = signaling_tx
                                .send(SignalingResponse::IceCandidate {
                                    candidate: candidate_init.candidate,
                                    sdp_mid: candidate_init.sdp_mid,
                                    sdp_mline_index: candidate_init.sdp_mline_index,
                                    username_fragment: candidate_init.username_fragment,
                                })
                                .await
                            {
                                warn!("Failed to send ICE candidate: {}", e);
                            } else {
                                debug!("ICE candidate sent to signaling service");
                            }
                        }
                        Err(e) => {
                            warn!("Failed to convert ICE candidate to JSON: {}", e);
                        }
                    }
                }
                None => {
                    // ICE gathering完了通知
                    info!("ICE candidate gathering complete");
                    if let Err(e) = signaling_tx
                        .send(SignalingResponse::IceCandidateComplete)
                        .await
                    {
                        warn!("Failed to send ICE candidate complete: {}", e);
                    } else {
                        debug!("ICE candidate complete sent to signaling service");
                    }
                }
            }
        })
    }));

    // LocalDescriptionとして設定
    pc.set_local_description(answer.clone())
        .await
        .context("Failed to set local description")?;

    // Answerをシグナリングサービスに送信
    if let Err(e) = signaling_tx
        .send(SignalingResponse::Answer { sdp: answer.sdp })
        .await
    {
        error!("Failed to send answer to signaling service: {}", e);
    } else {
        info!("Answer sent to signaling service");
    }

    // PeerConnection状態の監視
    let pc_for_state = pc.clone();
    let connection_ready_pc = connection_ready.clone();
    let video_stream_msg_tx_on_connect = video_stream_msg_tx.clone();
    pc_for_state.on_peer_connection_state_change(Box::new(move |state: RTCPeerConnectionState| {
        let connection_ready_pc = connection_ready_pc.clone();
        let video_stream_msg_tx_on_connect = video_stream_msg_tx_on_connect.clone();
        Box::pin(async move {
            match state {
                RTCPeerConnectionState::New => {
                    info!("PeerConnection state: New");
                }
                RTCPeerConnectionState::Connecting => {
                    info!("PeerConnection state: Connecting");
                    let was_ready = connection_ready_pc.load(Ordering::Relaxed);
                    connection_ready_pc.store(false, Ordering::Relaxed);
                    if was_ready {
                        info!("connection_ready flag set to false (PeerConnection Connecting)");
                    }
                }
                RTCPeerConnectionState::Connected => {
                    info!("PeerConnection state: Connected - Media stream should be active");
                    let was_ready = connection_ready_pc.load(Ordering::Relaxed);
                    connection_ready_pc.store(true, Ordering::Relaxed);
                    if !was_ready {
                        info!("connection_ready flag set to true (PeerConnection Connected)");
                    }
                    // 接続確立時に即座にキーフレーム送出を要求
                    let _ = video_stream_msg_tx_on_connect
                        .send(VideoStreamMessage::RequestKeyframe)
                        .await;
                }
                RTCPeerConnectionState::Disconnected => {
                    warn!("PeerConnection state: Disconnected - Connection lost");
                    let was_ready = connection_ready_pc.load(Ordering::Relaxed);
                    connection_ready_pc.store(false, Ordering::Relaxed);
                    if was_ready {
                        warn!("connection_ready flag set to false (PeerConnection Disconnected)");
                    }
                }
                RTCPeerConnectionState::Failed => {
                    error!("PeerConnection state: Failed - Connection failed");
                    let was_ready = connection_ready_pc.load(Ordering::Relaxed);
                    connection_ready_pc.store(false, Ordering::Relaxed);
                    if was_ready {
                        error!("connection_ready flag set to false (PeerConnection Failed)");
                    }
                }
                RTCPeerConnectionState::Closed => {
                    info!("PeerConnection state: Closed");
                    connection_ready_pc.store(false, Ordering::Relaxed);
                }
                RTCPeerConnectionState::Unspecified => {
                    debug!("PeerConnection state: Unspecified");
                }
            }
        })
    }));

    // ICE接続状態の監視
    let pc_for_ice = pc.clone();
    let connection_ready_ice = connection_ready.clone();
    let video_stream_msg_tx_ice = video_stream_msg_tx.clone();
    // 猶予期間中のフラグ（猶予期間中にConnectedに戻った場合、タイマーを無効化するため）
    let grace_period_active = Arc::new(AtomicBool::new(false));
    pc_for_ice.on_ice_connection_state_change(Box::new(move |state| {
        let connection_ready_ice = connection_ready_ice.clone();
        let video_stream_msg_tx_ice = video_stream_msg_tx_ice.clone();
        let grace_period_active = grace_period_active.clone();
        Box::pin(async move {
            match state {
                webrtc_rs::ice_transport::ice_connection_state::RTCIceConnectionState::New => {
                    info!("ICE connection state: New");
                }
                webrtc_rs::ice_transport::ice_connection_state::RTCIceConnectionState::Checking => {
                    info!("ICE connection state: Checking");
                    let was_ready = connection_ready_ice.load(Ordering::Relaxed);
                    connection_ready_ice.store(false, Ordering::Relaxed);
                    if was_ready {
                        info!("connection_ready flag set to false (ICE Checking)");
                    }
                }
                webrtc_rs::ice_transport::ice_connection_state::RTCIceConnectionState::Connected => {
                    info!("ICE connection state: Connected - ICE connection established");
                    // 猶予期間中にConnectedに戻った場合は、タイマーを無効化
                    grace_period_active.store(false, Ordering::Relaxed);
                    let was_ready = connection_ready_ice.load(Ordering::Relaxed);
                    connection_ready_ice.store(true, Ordering::Relaxed);
                    if !was_ready {
                        info!("connection_ready flag set to true (ICE Connected)");
                        // ICE接続確立時にもキーフレーム送出を要求
                        let _ = video_stream_msg_tx_ice
                            .send(VideoStreamMessage::RequestKeyframe)
                            .await;
                    }
                }
                webrtc_rs::ice_transport::ice_connection_state::RTCIceConnectionState::Completed => {
                    info!("ICE connection state: Completed - ICE gathering complete");
                    // 猶予期間中にCompletedになった場合も、タイマーを無効化
                    grace_period_active.store(false, Ordering::Relaxed);
                    let was_ready = connection_ready_ice.load(Ordering::Relaxed);
                    connection_ready_ice.store(true, Ordering::Relaxed);
                    if !was_ready {
                        info!("connection_ready flag set to true (ICE Completed)");
                    }
                }
                webrtc_rs::ice_transport::ice_connection_state::RTCIceConnectionState::Failed => {
                    error!("ICE connection state: Failed - ICE connection failed");
                    grace_period_active.store(false, Ordering::Relaxed);
                    connection_ready_ice.store(false, Ordering::Relaxed);
                }
                webrtc_rs::ice_transport::ice_connection_state::RTCIceConnectionState::Disconnected => {
                    warn!("ICE connection state: Disconnected - ICE connection lost");
                    let was_ready = connection_ready_ice.load(Ordering::Relaxed);
                    if was_ready {
                        // 猶予期間を設定（5秒）
                        grace_period_active.store(true, Ordering::Relaxed);
                        let connection_ready_grace = connection_ready_ice.clone();
                        let grace_period_active_grace = grace_period_active.clone();
                        tokio::spawn(async move {
                            tokio::time::sleep(Duration::from_secs(5)).await;
                            // 猶予期間が終了した時、まだ猶予期間中（Connectedに戻っていない）なら
                            // connection_readyをfalseにする
                            if grace_period_active_grace.load(Ordering::Relaxed) {
                                connection_ready_grace.store(false, Ordering::Relaxed);
                                warn!("connection_ready flag set to false (ICE Disconnected - grace period expired)");
                            }
                        });
                        warn!("ICE connection disconnected, starting 5-second grace period");
                    } else {
                        // 既にready=falseの場合は即座にfalseのまま
                        connection_ready_ice.store(false, Ordering::Relaxed);
                    }
                }
                webrtc_rs::ice_transport::ice_connection_state::RTCIceConnectionState::Closed => {
                    info!("ICE connection state: Closed");
                    grace_period_active.store(false, Ordering::Relaxed);
                    connection_ready_ice.store(false, Ordering::Relaxed);
                }
                webrtc_rs::ice_transport::ice_connection_state::RTCIceConnectionState::Unspecified => {
                    debug!("ICE connection state: Unspecified");
                }
            }
        })
    }));

    // Track受信のハンドラを設定
    let pc_for_track = pc.clone();
    pc_for_track.on_track(Box::new(move |track, _receiver, _transceiver| {
        Box::pin(async move {
            info!("Track received: {}", track.kind());
        })
    }));

    Ok(SetOfferResult {
        peer_connection: pc,
        video_track,
        video_sender: sender,
        audio_track,
        audio_sender,
    })
}

/// ICE candidate追加処理
pub async fn handle_add_ice_candidate(
    peer_connection: &Arc<RTCPeerConnection>,
    candidate: String,
    sdp_mid: Option<String>,
    sdp_mline_index: Option<u16>,
    username_fragment: Option<String>,
) -> Result<()> {
    debug!("AddIceCandidate received");
    let ice_candidate = RTCIceCandidateInit {
        candidate,
        sdp_mid,
        sdp_mline_index,
        username_fragment,
    };
    peer_connection
        .add_ice_candidate(ice_candidate)
        .await
        .context("Failed to add ICE candidate")?;
    debug!("ICE candidate added");
    Ok(())
}
