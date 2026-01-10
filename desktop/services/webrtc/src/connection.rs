use anyhow::{Context, Result};
use core_types::{
    AudioEncoderFactory, AudioFrame, DataChannelMessage, SignalingResponse, VideoCodec, VideoEncoderFactory,
};
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
use webrtc_rs::interceptor::registry::Registry;
use webrtc_rs::peer_connection::configuration::RTCConfiguration;
use webrtc_rs::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc_rs::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc_rs::peer_connection::RTCPeerConnection;
use webrtc_rs::rtcp::payload_feedbacks::full_intra_request::FullIntraRequest;
use webrtc_rs::rtcp::payload_feedbacks::picture_loss_indication::PictureLossIndication;
use webrtc_rs::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;
use webrtc_rs::rtp_transceiver::rtp_sender::RTCRtpSender;
use webrtc_rs::stats::StatsReportType;
use webrtc_rs::track::track_local::track_local_static_sample::TrackLocalStaticSample;
use webrtc_rs::track::track_local::TrackLocal;

use crate::track_writer::VideoTrackState;

/// m-line情報
#[derive(Clone)]
pub struct MLineInfo {
    pub mid: Option<String>,
    pub index: usize,
    pub media_type: String,
}

/// Answer SDPからm-line情報を解析
pub fn parse_answer_m_lines(answer_sdp: &str) -> Vec<MLineInfo> {
    let mut m_lines = Vec::new();
    let lines: Vec<&str> = answer_sdp.lines().collect();
    let mut m_line_index = 0;

    for (i, line) in lines.iter().enumerate() {
        if line.starts_with("m=") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if !parts.is_empty() {
                let media_type = parts[0].trim_start_matches("m=").to_string();

                // このm-lineのmidを探す（次のm=または終端まで）
                let mut mid = None;
                for next_line in lines.iter().skip(i + 1) {
                    if next_line.starts_with("m=") {
                        break; // 次のm-lineに到達
                    }
                    if next_line.starts_with("a=mid:") {
                        mid = Some(next_line.trim_start_matches("a=mid:").to_string());
                        break;
                    }
                }

                m_lines.push(MLineInfo {
                    mid,
                    index: m_line_index,
                    media_type,
                });
                m_line_index += 1;
            }
        }
    }

    m_lines
}

/// RTCIceCandidateから完全なSDP candidate文字列を生成
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
    pub video_track_state: VideoTrackState,
    pub encode_job_slot: Arc<core_types::EncodeJobSlot>,
    pub encode_result_rx: tokio::sync::mpsc::UnboundedReceiver<core_types::EncodeResult>,
    pub audio_track: Option<Arc<TrackLocalStaticSample>>,
    pub audio_encode_result_rx:
        Option<tokio::sync::mpsc::UnboundedReceiver<core_types::AudioEncodeResult>>,
    pub audio_sender: Option<Arc<RTCRtpSender>>,
}

/// SetOfferメッセージを処理
pub async fn handle_set_offer(
    sdp: String,
    _codec: Option<VideoCodec>,
    encoder_factory: Arc<dyn VideoEncoderFactory>,
    selected_codec: VideoCodec,
    signaling_tx: mpsc::Sender<SignalingResponse>,
    data_channel_tx: mpsc::Sender<DataChannelMessage>,
    connection_ready: Arc<AtomicBool>,
    keyframe_tx: mpsc::UnboundedSender<()>,
    audio_frame_rx: Option<mpsc::Receiver<AudioFrame>>,
    audio_encoder_factory: Option<Arc<dyn AudioEncoderFactory>>,
) -> Result<SetOfferResult> {
    info!("SetOffer received, generating answer");

    // webrtc-rsのAPIを初期化
    let mut m = MediaEngine::default();
    m.register_default_codecs()?;

    let mut registry = Registry::new();
    registry = register_default_interceptors(registry, &mut m)?;

    // ループバック候補を含める（同一ホスト内接続を確実にするため）
    let mut setting_engine = SettingEngine::default();
    setting_engine.set_include_loopback_candidate(true);

    let api = APIBuilder::new()
        .with_media_engine(m)
        .with_setting_engine(setting_engine)
        .with_interceptor_registry(registry)
        .build();

    // ICE設定（ホストオンリー）
    let config = RTCConfiguration {
        ice_servers: vec![],
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

    // Video trackを作成して追加（encoderが提供するコーデックに合わせる）
    let mime_type = codec_to_mime_type(selected_codec);
    info!("Using video codec: {:?}", selected_codec);

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
    let mut audio_encode_result_rx: Option<
        tokio::sync::mpsc::UnboundedReceiver<core_types::AudioEncodeResult>,
    > = None;
    let mut audio_sender: Option<Arc<RTCRtpSender>> = None;

    info!(
        "Audio input available: {}",
        audio_frame_rx.is_some() && audio_encoder_factory.is_some()
    );

    if let (Some(mut audio_frame_rx), Some(audio_encoder_factory)) =
        (audio_frame_rx, audio_encoder_factory)
    {
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

        // Audio用のRTCPドレインループ
        let audio_sender_for_rtcp = audio_sender_local.clone();
        tokio::spawn(async move {
            let mut rtcp_buf = vec![0u8; 1500];
            while let Ok((_, _)) = audio_sender_for_rtcp.read(&mut rtcp_buf).await {}
        });

        // 音声エンコーダーをセットアップ
        let (audio_encoder_tx, audio_result_rx) = audio_encoder_factory.setup();

        // 音声フレームをエンコーダーに転送するタスクをスポーン
        tokio::spawn(async move {
            while let Some(frame) = audio_frame_rx.recv().await {
                if audio_encoder_tx.send(frame).await.is_err() {
                    debug!("Audio encoder channel closed");
                    break;
                }
            }
        });

        audio_encode_result_rx = Some(audio_result_rx);
        audio_track = Some(audio_track_local);
        audio_sender = Some(audio_sender_local);
    }

    // RTCP 受信ループを開始し、PLI/FIR を受けたらキーフレーム再送を要求
    let keyframe_tx_rtcp = keyframe_tx.clone();
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
                            let _ = keyframe_tx_rtcp.send(());
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

    // RTCP をドレインするループ（NACK 等を確実に処理）
    let sender_for_rtcp_drain = sender.clone();
    tokio::spawn(async move {
        let mut rtcp_buf = vec![0u8; 1500];
        while let Ok((_, _)) = sender_for_rtcp_drain.read(&mut rtcp_buf).await {}
    });

    // 送信統計 状態を定期ログ（5秒間隔）
    let pc_for_log = pc.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;

            // sender/get_stats 相当の情報（OutboundRTP）を PeerConnection 経由で確認
            let stats = pc_for_log.get_stats().await;
            let mut video_logged = false;
            let mut audio_logged = false;
            for report in stats.reports.values() {
                if let StatsReportType::OutboundRTP(out) = report {
                    if out.kind == "video" {
                        info!(
                            "video sender stats: ssrc={} bytes_sent={} packets_sent={} nack={} pli={:?} fir={:?}",
                            out.ssrc,
                            out.bytes_sent,
                            out.packets_sent,
                            out.nack_count,
                            out.pli_count,
                            out.fir_count,
                        );
                        video_logged = true;
                    } else if out.kind == "audio" {
                        info!(
                            "audio sender stats: ssrc={} bytes_sent={} packets_sent={}",
                            out.ssrc,
                            out.bytes_sent,
                            out.packets_sent,
                        );
                        audio_logged = true;
                    }
                }
            }
            if !video_logged {
                info!("video sender stats: outbound RTP not found in get_stats");
            }
            if !audio_logged {
                debug!("audio sender stats: outbound RTP not found in get_stats");
            }
        }
    });

    // エンコードワーカーを起動
    let (encode_job_slot, encode_result_rx) = encoder_factory.setup();

    // SPS/PPSの送出はLocalDescription設定後、最初のフレーム処理時に実行
    // 初期値はfalseに設定（交渉完了後に送信）
    // 解像度は最初のフレームが来たときに設定される
    let video_track_state = VideoTrackState {
        track: video_track,
        width: 0,
        height: 0,
        keyframe_sent: false,
        encoder_factory: encoder_factory.clone(),
    };

    // DataChannelハンドラを設定
    let dc_tx = data_channel_tx.clone();
    pc.on_data_channel(Box::new(move |dc: Arc<RTCDataChannel>| {
        let dc_tx = dc_tx.clone();
        Box::pin(async move {
            let label = dc.label();
            let label_str = label.to_string();
            info!("DataChannel opened: {}", label_str);

            let dc_tx_on_msg = dc_tx.clone();
            dc.on_message(Box::new(move |msg: RTCDataChannelMessage| {
                let dc_tx_on_msg = dc_tx_on_msg.clone();
                Box::pin(async move {
                    if msg.is_string {
                        if let Ok(text) = String::from_utf8(msg.data.to_vec()) {
                            match serde_json::from_str::<DataChannelMessage>(&text) {
                                Ok(parsed) => {
                                    if let Err(e) = dc_tx_on_msg.send(parsed).await {
                                        warn!("Failed to forward data channel message: {}", e);
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

            dc.on_close(Box::new(move || {
                let label_str = label_str.clone();
                Box::pin(async move {
                    info!("DataChannel closed: {}", label_str);
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

    // Answer SDPからm-line情報を解析（ICEハンドラ設定に使用）
    let m_lines = parse_answer_m_lines(&answer.sdp);
    info!("Answer SDP parsed: {} m-lines", m_lines.len());
    info!(
        "Answer SDP includes mime {}: {}",
        mime_type,
        answer.sdp.contains(&mime_type)
    );

    // ICE candidateのイベントハンドラを LocalDescription 設定前に登録して、
    // 初期ホスト候補を取りこぼさないようにする
    let signaling_tx_ice = signaling_tx.clone();
    let answer_m_lines = m_lines.clone();
    pc.on_ice_candidate(Box::new(move |candidate: Option<RTCIceCandidate>| {
        let signaling_tx = signaling_tx_ice.clone();
        let m_lines = answer_m_lines.clone();
        Box::pin(async move {
            if let Some(candidate) = candidate {
                // RTCIceCandidateから完全なSDP candidate文字列を生成
                let candidate_str = format_ice_candidate(&candidate);

                // candidateのcomponentからm-lineを特定
                // component 1 = RTP, component 2 = RTCP
                // BUNDLEにより全メディアが同じトランスポートを共有するため、
                // 最初のm-lineを使用
                let sdp_mid = if candidate.component == 1 {
                    m_lines.first().and_then(|m| m.mid.clone())
                } else {
                    None
                };

                let sdp_mline_index = if candidate.component == 1 {
                    m_lines.first().map(|m| m.index as u16)
                } else {
                    None
                };

                info!(
                    "ICE candidate: {} (mid: {:?}, mline_index: {:?})",
                    candidate_str, sdp_mid, sdp_mline_index
                );

                if let Err(e) = signaling_tx
                    .send(SignalingResponse::IceCandidate {
                        candidate: candidate_str,
                        sdp_mid,
                        sdp_mline_index,
                    })
                    .await
                {
                    warn!("Failed to send ICE candidate: {}", e);
                } else {
                    debug!("ICE candidate sent to signaling service");
                }
            }
        })
    }));

    // LocalDescriptionとして設定
    pc.set_local_description(answer.clone())
        .await
        .context("Failed to set local description")?;

    // 念のため送信開始を明示的にトリガー（start_rtp_senders 依存の補完）
    // すでに送信開始済みの場合は ErrRTPSenderSendAlreadyCalled になるので debug ログのみ
    let sender_for_start = sender.clone();
    tokio::spawn(async move {
        let params = sender_for_start.get_parameters().await;
        match sender_for_start.send(&params).await {
            Ok(_) => info!("Video RTCRtpSender::send() invoked explicitly"),
            Err(e) => debug!("Video RTCRtpSender::send() explicit call returned: {}", e),
        }
    });

    // Audio senderに対しても明示的に送信開始をトリガー
    if let Some(ref audio_sender_local) = audio_sender {
        let audio_sender_for_start = audio_sender_local.clone();
        tokio::spawn(async move {
            let params = audio_sender_for_start.get_parameters().await;
            match audio_sender_for_start.send(&params).await {
                Ok(_) => info!("Audio RTCRtpSender::send() invoked explicitly"),
                Err(e) => debug!("Audio RTCRtpSender::send() explicit call returned: {}", e),
            }
        });
    }

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
    let keyframe_tx_on_connect = keyframe_tx.clone();
    pc_for_state.on_peer_connection_state_change(Box::new(move |state: RTCPeerConnectionState| {
        let connection_ready_pc = connection_ready_pc.clone();
        let keyframe_tx_on_connect = keyframe_tx_on_connect.clone();
        Box::pin(async move {
            match state {
                RTCPeerConnectionState::New => {
                    info!("PeerConnection state: New");
                }
                RTCPeerConnectionState::Connecting => {
                    info!("PeerConnection state: Connecting");
                    connection_ready_pc.store(false, Ordering::Relaxed);
                }
                RTCPeerConnectionState::Connected => {
                    info!("PeerConnection state: Connected - Media stream should be active");
                    connection_ready_pc.store(true, Ordering::Relaxed);
                    // 接続確立時に即座にキーフレーム送出を要求
                    let _ = keyframe_tx_on_connect.send(());
                }
                RTCPeerConnectionState::Disconnected => {
                    warn!("PeerConnection state: Disconnected - Connection lost");
                    connection_ready_pc.store(false, Ordering::Relaxed);
                }
                RTCPeerConnectionState::Failed => {
                    error!("PeerConnection state: Failed - Connection failed");
                    connection_ready_pc.store(false, Ordering::Relaxed);
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
    pc_for_ice.on_ice_connection_state_change(Box::new(move |state| {
        let connection_ready_ice = connection_ready_ice.clone();
        Box::pin(async move {
            match state {
                webrtc_rs::ice_transport::ice_connection_state::RTCIceConnectionState::New => {
                    info!("ICE connection state: New");
                }
                webrtc_rs::ice_transport::ice_connection_state::RTCIceConnectionState::Checking => {
                    info!("ICE connection state: Checking");
                    connection_ready_ice.store(false, Ordering::Relaxed);
                }
                webrtc_rs::ice_transport::ice_connection_state::RTCIceConnectionState::Connected => {
                    info!("ICE connection state: Connected - ICE connection established");
                    connection_ready_ice.store(true, Ordering::Relaxed);
                }
                webrtc_rs::ice_transport::ice_connection_state::RTCIceConnectionState::Completed => {
                    info!("ICE connection state: Completed - ICE gathering complete");
                    connection_ready_ice.store(true, Ordering::Relaxed);
                }
                webrtc_rs::ice_transport::ice_connection_state::RTCIceConnectionState::Failed => {
                    error!("ICE connection state: Failed - ICE connection failed");
                    connection_ready_ice.store(false, Ordering::Relaxed);
                }
                webrtc_rs::ice_transport::ice_connection_state::RTCIceConnectionState::Disconnected => {
                    warn!("ICE connection state: Disconnected - ICE connection lost");
                    connection_ready_ice.store(false, Ordering::Relaxed);
                }
                webrtc_rs::ice_transport::ice_connection_state::RTCIceConnectionState::Closed => {
                    info!("ICE connection state: Closed");
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
        video_track_state,
        encode_job_slot,
        encode_result_rx,
        audio_track,
        audio_encode_result_rx,
        audio_sender,
    })
}

/// ICE candidate追加処理
pub async fn handle_add_ice_candidate(
    peer_connection: &Arc<RTCPeerConnection>,
    candidate: String,
    sdp_mid: Option<String>,
    sdp_mline_index: Option<u16>,
) -> Result<()> {
    debug!("AddIceCandidate received");
    let ice_candidate = RTCIceCandidateInit {
        candidate,
        sdp_mid,
        sdp_mline_index,
        username_fragment: None,
    };
    peer_connection
        .add_ice_candidate(ice_candidate)
        .await
        .context("Failed to add ICE candidate")?;
    debug!("ICE candidate added");
    Ok(())
}
