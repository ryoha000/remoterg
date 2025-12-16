use anyhow::{bail, Context, Result};
use core_types::{EncodeJob, EncodeResult, VideoCodec, VideoEncoderFactory};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tracing::{debug, error, info, span, warn, Level};
use webrtc_rs::api::interceptor_registry::register_default_interceptors;
use webrtc_rs::api::media_engine::{MediaEngine, MIME_TYPE_H264, MIME_TYPE_VP8, MIME_TYPE_VP9};
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

use core_types::{DataChannelMessage, Frame, SignalingResponse, WebRtcMessage};

/// WebRTCサービス
pub struct WebRtcService {
    frame_rx: mpsc::Receiver<Frame>,
    message_rx: mpsc::Receiver<WebRtcMessage>,
    signaling_tx: mpsc::Sender<SignalingResponse>,
    data_channel_tx: mpsc::Sender<DataChannelMessage>,
    encoder_factories: HashMap<VideoCodec, Arc<dyn VideoEncoderFactory>>,
}

/// Video trackとエンコーダーの状態
struct VideoTrackState {
    track: Arc<TrackLocalStaticSample>,
    width: u32,
    height: u32,
    keyframe_sent: bool, // 初期キーフレーム送信済みか
}

/// Answer SDPのm-line情報
#[allow(dead_code)]
struct AnswerSdpInfo {
    sdp: String,
    m_lines: Vec<MLineInfo>,
}

/// m-line情報
#[derive(Clone)]
struct MLineInfo {
    mid: Option<String>,
    index: usize,
    media_type: String,
}

/// Answer SDPからm-line情報を解析
fn parse_answer_m_lines(answer_sdp: &str) -> Vec<MLineInfo> {
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
fn format_ice_candidate(candidate: &RTCIceCandidate) -> String {
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

fn codec_to_mime_type(codec: VideoCodec) -> String {
    match codec {
        VideoCodec::H264 => MIME_TYPE_H264.to_owned(),
        VideoCodec::Vp8 => MIME_TYPE_VP8.to_string(),
        VideoCodec::Vp9 => MIME_TYPE_VP9.to_string(),
    }
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

        for codec in [VideoCodec::Vp9, VideoCodec::Vp8, VideoCodec::H264] {
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

        let mut peer_connection: Option<Arc<RTCPeerConnection>> = None;
        let mut video_track_state: Option<VideoTrackState> = None;
        let mut encode_job_tx: Option<std::sync::mpsc::Sender<EncodeJob>> = None;
        let mut encode_result_rx: Option<tokio::sync::mpsc::UnboundedReceiver<EncodeResult>> = None;

        let mut frame_count: u64 = 0;
        let mut last_frame_ts: Option<u64> = None;
        let mut last_frame_log = Instant::now();
        let mut frames_received: u64 = 0;
        let mut frames_dropped_not_ready: u64 = 0;
        let mut frames_dropped_no_track: u64 = 0;
        let mut frames_queued: u64 = 0;
        let mut last_perf_log = Instant::now();

        loop {
            tokio::select! {
                // フレーム受信
                frame = self.frame_rx.recv() => {
                    match frame {
                        Some(frame) => {
                            frames_received += 1;
                            let pipeline_start = Instant::now();
                            let interarrival_ms = last_frame_ts
                                .map(|prev| frame.timestamp.saturating_sub(prev))
                                .unwrap_or(0);

                            debug!(
                                "Received frame: {}x{} (since_last={}ms)",
                                frame.width, frame.height, interarrival_ms
                            );

                            // ICE/DTLS 接続完了まで映像送出を保留
                            if !connection_ready.load(Ordering::Relaxed) {
                                frames_dropped_not_ready += 1;
                                if frames_dropped_not_ready % 30 == 0 {
                                    debug!("Connection not ready yet, dropped {} frames", frames_dropped_not_ready);
                                }
                                continue;
                            }

                            // フレーム処理全体を span で計測
                            let process_frame_span = span!(
                                Level::DEBUG,
                                "process_frame",
                                width = frame.width,
                                height = frame.height
                            );
                            let _process_frame_guard = process_frame_span.enter();

                            // Video trackが存在する場合、エンコードワーカーへジョブを送信
                            if let (Some(track_state), Some(job_tx)) =
                                (video_track_state.as_mut(), encode_job_tx.as_ref())
                            {
                                // capture側のタイムスタンプ差分からフレーム間隔を推定（デフォルト22ms≒45fps）
                                let frame_duration = if let Some(prev) = last_frame_ts {
                                    let delta_ms = frame.timestamp.saturating_sub(prev).max(1);
                                    Duration::from_millis(delta_ms)
                                } else {
                                    Duration::from_millis(22)
                                };
                                last_frame_ts = Some(frame.timestamp);

                                // 解像度変更はワーカー内で再生成するが、ログは出しておく
                                if track_state.width != frame.width || track_state.height != frame.height {
                                    if track_state.width == 0 && track_state.height == 0 {
                                        info!(
                                            "Observed first frame {}x{} (encoder will initialize in worker)",
                                            frame.width, frame.height
                                        );
                                    } else {
                                        info!(
                                            "Observed frame resize {}x{} -> {}x{} (encoder will recreate in worker)",
                                            track_state.width, track_state.height, frame.width, frame.height
                                        );
                                    }
                                    // トラック状態は最新解像度を保持（SPS/PPS再送時に使用）
                                    track_state.width = frame.width;
                                    track_state.height = frame.height;
                                }

                                // エンコードジョブ送信を span で計測
                                let queue_encode_job_span = span!(
                                    Level::DEBUG,
                                    "queue_encode_job"
                                );
                                let _queue_encode_job_guard = queue_encode_job_span.enter();
                                let job_send_start = Instant::now();
                                let send_result = job_tx.send(EncodeJob {
                                    width: frame.width,
                                    height: frame.height,
                                    rgba: frame.data,
                                    duration: frame_duration,
                                    enqueue_at: pipeline_start,
                                });
                                let job_send_dur = job_send_start.elapsed();
                                drop(_queue_encode_job_guard);

                                match send_result {
                                    Ok(_) => {
                                        frames_queued += 1;
                                        if job_send_dur.as_millis() > 10 {
                                        warn!(
                                            "Encode job send took {}ms (queue may be full)",
                                            job_send_dur.as_millis()
                                        );
                                        }
                                    }
                                    Err(e) => {
                                        warn!("Failed to queue encode job: {}", e);
                                    }
                                }
                            } else {
                                frames_dropped_no_track += 1;
                                if frames_dropped_no_track % 30 == 0 {
                                    debug!("Video track not ready or encoder worker not available, dropped {} frames", frames_dropped_no_track);
                                }
                            }

                            drop(_process_frame_guard);

                            // パフォーマンス統計を定期的に出力
                            if last_perf_log.elapsed().as_secs_f32() >= 5.0 {
                                let elapsed_sec = last_perf_log.elapsed().as_secs_f32();
                                let receive_fps = frames_received as f32 / elapsed_sec;
                                let queue_fps = frames_queued as f32 / elapsed_sec;
                                info!(
                                    "Frame processing stats (last {}s): received={} ({:.1} fps), queued={} ({:.1} fps), dropped_not_ready={}, dropped_no_track={}",
                                    elapsed_sec,
                                    frames_received,
                                    receive_fps,
                                    frames_queued,
                                    queue_fps,
                                    frames_dropped_not_ready,
                                    frames_dropped_no_track
                                );
                                frames_received = 0;
                                frames_queued = 0;
                                frames_dropped_not_ready = 0;
                                frames_dropped_no_track = 0;
                                last_perf_log = Instant::now();
                            }
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
                            if result.is_keyframe {
                                track_state.keyframe_sent = true;
                            }

                            use webrtc_media::Sample;
                            use bytes::Bytes;

                            let sample_size = result.sample_data.len();
                            let sample = Sample {
                                data: Bytes::from(result.sample_data),
                                duration: result.duration,
                                ..Default::default()
                            };

                            // サンプル書き込みを span で計測
                            let write_sample_span = span!(
                                Level::DEBUG,
                                "write_sample",
                                width = result.width,
                                height = result.height,
                                sample_size = sample_size,
                                is_keyframe = result.is_keyframe
                            );
                            let _write_sample_guard = write_sample_span.enter();
                            match track_state.track.write_sample(&sample).await {
                                Ok(_) => {
                                    drop(_write_sample_guard);
                                    frame_count += 1;
                                    let elapsed = last_frame_log.elapsed();
                                    if elapsed.as_secs_f32() >= 5.0 {
                                        info!("Video frames sent: {} (last {}s)", frame_count, elapsed.as_secs());
                                        frame_count = 0;
                                        last_frame_log = Instant::now();
                                    }
                                }
                                Err(e) => {
                                    drop(_write_sample_guard);
                                    error!("Failed to write sample to track: {}", e);
                                }
                            }
                        } else {
                            debug!("Received encoded frame but video track is not ready");
                        }
                    }
                }
                // PLI/FIR によるキーフレーム再送要求
                keyframe_req = keyframe_rx.recv() => {
                    if keyframe_req.is_some() {
                        if let Some(ref mut track_state) = video_track_state {
                            info!("Keyframe requested via RTCP; awaiting next keyframe from encoder");
                            track_state.keyframe_sent = false;
                        } else {
                            debug!("Keyframe requested but video track is not ready yet");
                        }
                    }
                }
                // メッセージ受信
                msg = self.message_rx.recv() => {
                    match msg {
                        Some(WebRtcMessage::SetOffer { sdp, codec }) => {
                            info!("SetOffer received, generating answer");

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
                            // ICE設定（ホストオンリー）
                            let config = RTCConfiguration {
                                ice_servers: vec![],
                                ..Default::default()
                            };

                            // PeerConnectionを作成
                            let pc = Arc::new(api.new_peer_connection(config).await
                                .context("Failed to create peer connection")?);

                            // OfferをRemoteDescriptionとして設定
                            let offer = RTCSessionDescription::offer(sdp)
                                .context("Failed to parse offer SDP")?;
                            info!("Offer SDP received:\n{}", offer.sdp);
                            pc.set_remote_description(offer).await
                                .context("Failed to set remote description")?;

                            // Video trackを作成して追加（encoderが提供するコーデックに合わせる）
                            let codec = selected_codec;
                            let mime_type = codec_to_mime_type(codec);
                            info!("Using video codec: {:?}", codec);

                            let video_track = Arc::new(TrackLocalStaticSample::new(
                                RTCRtpCodecCapability {
                                    mime_type: mime_type.clone(),
                                    ..Default::default()
                                },
                                "video".to_string(),
                                "stream".to_string(),
                            ));

                            // Transceiverを追加（sendonly）
                            let sender: Arc<RTCRtpSender> = pc.add_track(video_track.clone() as Arc<dyn TrackLocal + Send + Sync>)
                                .await
                                .context("Failed to add video track")?;

                            info!("Video track added to peer connection");

                            // RTCP 受信ループを開始し、PLI/FIR を受けたらキーフレーム再送を要求
                            let keyframe_tx_rtcp = keyframe_tx.clone();
                            let sender_for_rtcp = sender.clone();
                            tokio::spawn(async move {
                                loop {
                                    match sender_for_rtcp.read_rtcp().await {
                                        Ok((pkts, _)) => {
                                            for pkt in pkts {
                                                if pkt.as_any().downcast_ref::<PictureLossIndication>().is_some()
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

                            // 送信トラックのパラメータ・送信統計・transceiver 状態を定期ログ（5秒間隔）
                            let sender_for_log = sender.clone();
                            let pc_for_log = pc.clone();
                            tokio::spawn(async move {
                                loop {
                                    tokio::time::sleep(Duration::from_secs(5)).await;

                                    // パラメータ（webrtc-rs 0.14 では get_parameters は Result ではなく値を返す）
                                    let params = sender_for_log.get_parameters().await;
                                    let ssrcs: Vec<_> = params.encodings.iter().map(|e| e.ssrc).collect();
                                    let pts: Vec<_> = params
                                        .rtp_parameters
                                        .codecs
                                        .iter()
                                        .map(|c| (c.payload_type, c.capability.mime_type.clone()))
                                        .collect();
                                    info!("sender params: ssrcs={:?}, codecs={:?}", ssrcs, pts);

                                    // sender/get_stats 相当の情報（OutboundRTP）を PeerConnection 経由で確認
                                    let stats = pc_for_log.get_stats().await;
                                    let mut outbound_logged = false;
                                    for report in stats.reports.values() {
                                        if let StatsReportType::OutboundRTP(out) = report {
                                            if out.kind == "video" {
                                                info!(
                                                    "sender stats: ssrc={} bytes_sent={} packets_sent={} nack={} pli={:?} fir={:?}",
                                                    out.ssrc,
                                                    out.bytes_sent,
                                                    out.packets_sent,
                                                    out.nack_count,
                                                    out.pli_count,
                                                    out.fir_count,
                                                );
                                                outbound_logged = true;
                                            }
                                        }
                                    }
                                    if !outbound_logged {
                                        info!("sender stats: outbound video RTP not found in get_stats");
                                    }

                                    // transceiver の希望方向・現在方向を確認
                                    let transceivers = pc_for_log.get_transceivers().await;
                                    for (idx, t) in transceivers.iter().enumerate() {
                                        info!(
                                            "transceiver[{}]: mid={:?} kind={:?} direction={:?} current_direction={:?}",
                                            idx,
                                            t.mid(),
                                            t.kind(),
                                            t.direction(),
                                            t.current_direction()
                                        );
                                    }
                                }
                            });

                            // エンコードワーカーを起動
                            let (job_tx, res_rx) = encoder_factory.setup();
                            encode_job_tx = Some(job_tx);
                            encode_result_rx = Some(res_rx);

                            // SPS/PPSの送出はLocalDescription設定後、最初のフレーム処理時に実行
                            // 初期値はfalseに設定（交渉完了後に送信）
                            // 解像度は最初のフレームが来たときに設定される
                            video_track_state = Some(VideoTrackState {
                                track: video_track,
                                width: 0,
                                height: 0,
                                keyframe_sent: false,
                            });

                            // DataChannelハンドラを設定
                            let dc_tx = self.data_channel_tx.clone();
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
                            let answer = pc.create_answer(None).await
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
                            let signaling_tx = self.signaling_tx.clone();
                            let answer_m_lines = m_lines.clone();
                            pc.on_ice_candidate(Box::new(move |candidate: Option<RTCIceCandidate>| {
                                let signaling_tx = signaling_tx.clone();
                                let m_lines = answer_m_lines.clone();
                                Box::pin(async move {
                                    if let Some(candidate) = candidate {
                                        // RTCIceCandidateから完全なSDP candidate文字列を生成
                                        let candidate_str = format_ice_candidate(&candidate);

                                        // candidateのcomponentからm-lineを特定
                                        // component 1 = RTP, component 2 = RTCP
                                        let sdp_mid = if candidate.component == 1 {
                                            m_lines.iter()
                                                .find(|m| m.media_type == "video")
                                                .and_then(|m| m.mid.clone())
                                        } else {
                                            None
                                        };

                                        let sdp_mline_index = if candidate.component == 1 {
                                            m_lines.iter()
                                                .find(|m| m.media_type == "video")
                                                .map(|m| m.index as u16)
                                        } else {
                                            None
                                        };

                                        info!("ICE candidate: {} (mid: {:?}, mline_index: {:?})",
                                            candidate_str, sdp_mid, sdp_mline_index);

                                        if let Err(e) = signaling_tx.send(SignalingResponse::IceCandidate {
                                            candidate: candidate_str,
                                            sdp_mid,
                                            sdp_mline_index,
                                        }).await {
                                            warn!("Failed to send ICE candidate: {}", e);
                                        } else {
                                            debug!("ICE candidate sent to signaling service");
                                        }
                                    }
                                })
                            }));

                            // LocalDescriptionとして設定
                            pc.set_local_description(answer.clone()).await
                                .context("Failed to set local description")?;

                            // 念のため送信開始を明示的にトリガー（start_rtp_senders 依存の補完）
                            // すでに送信開始済みの場合は ErrRTPSenderSendAlreadyCalled になるので debug ログのみ
                            let sender_for_start = sender.clone();
                            tokio::spawn(async move {
                                let params = sender_for_start.get_parameters().await;
                                match sender_for_start.send(&params).await {
                                    Ok(_) => info!("RTCRtpSender::send() invoked explicitly"),
                                    Err(e) => debug!("RTCRtpSender::send() explicit call returned: {}", e),
                                }
                            });

                            // Answerをシグナリングサービスに送信
                            if let Err(e) = self.signaling_tx.send(SignalingResponse::Answer {
                                sdp: answer.sdp
                            }).await {
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

                            peer_connection = Some(pc.clone());
                        }
                        Some(WebRtcMessage::AddIceCandidate { candidate, sdp_mid, sdp_mline_index }) => {
                            debug!("AddIceCandidate received");
                            if let Some(ref pc) = peer_connection {
                                let ice_candidate = RTCIceCandidateInit {
                                    candidate,
                                    sdp_mid,
                                    sdp_mline_index,
                                    username_fragment: None,
                                };
                                if let Err(e) = pc.add_ice_candidate(ice_candidate).await {
                                    error!("Failed to add ICE candidate: {}", e);
                                } else {
                                    debug!("ICE candidate added");
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
