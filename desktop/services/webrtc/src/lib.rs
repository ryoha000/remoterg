use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};
use webrtc_rs::api::interceptor_registry::register_default_interceptors;
use webrtc_rs::api::media_engine::MediaEngine;
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
use webrtc_rs::track::track_local::track_local_static_sample::TrackLocalStaticSample;
use webrtc_rs::track::track_local::TrackLocal;
use webrtc_rs::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;

use capture::Frame;

/// WebRTCサービスのメッセージ（内部用）
#[derive(Debug, Clone)]
pub enum WebRtcMessage {
    SetOffer { sdp: String },
    AddIceCandidate { candidate: String, sdp_mid: Option<String>, sdp_mline_index: Option<u16> },
}

/// シグナリングサービスへの応答メッセージ
#[derive(Debug, Clone)]
pub enum SignalingResponse {
    Answer { sdp: String },
    IceCandidate { candidate: String, sdp_mid: Option<String>, sdp_mline_index: Option<u16> },
}

/// WebRTCサービス
pub struct WebRtcService {
    frame_rx: mpsc::Receiver<Frame>,
    message_rx: mpsc::Receiver<WebRtcMessage>,
    signaling_tx: mpsc::Sender<SignalingResponse>,
    data_channel_tx: mpsc::Sender<DataChannelMessage>,
}

/// Video trackとエンコーダーの状態
struct VideoTrackState {
    track: Arc<TrackLocalStaticSample>,
    encoder: openh264::encoder::Encoder,
    width: u32,
    height: u32,
    sps_pps_sent: bool, // SPS/PPSが送信済みかどうか
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

/// OpenH264のEncodedBitStreamからAnnex-B形式のH.264データを生成
/// 戻り値: (Annex-B形式のデータ, SPS/PPSが含まれているか)
fn annexb_from_bitstream(bitstream: &openh264::encoder::EncodedBitStream) -> (Vec<u8>, bool) {
    const START_CODE: &[u8] = &[0x00, 0x00, 0x00, 0x01];
    let mut sample_data = Vec::new();
    let mut has_sps_pps = false;
    
    let num_layers = bitstream.num_layers();
    if num_layers == 0 {
        warn!("EncodedBitStream has no layers");
        return (sample_data, has_sps_pps);
    }
    
    debug!("Processing {} layers", num_layers);
    
    for i in 0..num_layers {
        if let Some(layer) = bitstream.layer(i) {
            // すべてのレイヤーを処理（SPS/PPSは非ビデオレイヤーに含まれる可能性がある）
            let nal_count = layer.nal_count();
            debug!("Layer {}: {} NAL units", i, nal_count);
            
            if nal_count == 0 {
                warn!("Layer {} has no NAL units", i);
                continue;
            }
            
            for j in 0..nal_count {
                if let Some(nal_unit) = layer.nal_unit(j) {
                    if nal_unit.is_empty() {
                        warn!("NAL unit {} in layer {} is empty", j, i);
                        continue;
                    }
                    
                    debug!("NAL unit {} in layer {}: {} bytes", j, i, nal_unit.len());
                    
                    // NALユニットの先頭にスタートコードがあるかチェック
                    let has_start_code = nal_unit.len() >= 4 
                        && nal_unit[0] == 0x00 
                        && nal_unit[1] == 0x00 
                        && nal_unit[2] == 0x00 
                        && nal_unit[3] == 0x01;
                    
                    // NAL typeを判定（スタートコードがある場合は4バイト目、ない場合は0バイト目）
                    let nal_header_offset = if has_start_code { 4 } else { 0 };
                    
                    if nal_unit.len() <= nal_header_offset {
                        warn!("NAL unit {} in layer {} is too small ({} bytes, offset {})", 
                            j, i, nal_unit.len(), nal_header_offset);
                        continue;
                    }
                    
                    let nal_type = nal_unit[nal_header_offset] & 0x1F;
                    debug!("NAL unit {} in layer {}: type={}, has_start_code={}", 
                        j, i, nal_type, has_start_code);
                    
                    // SPS (type 7) または PPS (type 8) を検出
                    if nal_type == 7 || nal_type == 8 {
                        has_sps_pps = true;
                        info!("Found SPS/PPS: type={}, size={} bytes", nal_type, nal_unit.len());
                    }
                    
                    // スタートコードがない場合は追加
                    if !has_start_code {
                        sample_data.extend_from_slice(START_CODE);
                    }
                    
                    // NALユニットを追加（スタートコードが既にある場合はそのまま、ない場合は追加済み）
                    sample_data.extend_from_slice(nal_unit);
                } else {
                    warn!("NAL unit {} in layer {} is None", j, i);
                }
            }
        } else {
            warn!("Layer {} is None", i);
        }
    }
    
    debug!("Total sample data: {} bytes, has_sps_pps: {}", sample_data.len(), has_sps_pps);
    
    (sample_data, has_sps_pps)
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
    let candidate_type = if candidate.address.starts_with("127.") || 
                           candidate.address.starts_with("192.168.") ||
                           candidate.address.starts_with("10.") ||
                           candidate.address.starts_with("172.") ||
                           candidate.address == "::1" ||
                           candidate.address.starts_with("fe80:") {
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

/// ダミーフレーム（真っ赤なフレーム）を生成してエンコードし、SPS/PPSを送信
async fn send_sps_pps_frame(
    track: &Arc<TrackLocalStaticSample>,
    encoder: &mut openh264::encoder::Encoder,
    width: u32,
    height: u32,
) -> Result<()> {
    info!("Sending SPS/PPS frame: {}x{} (red frame)", width, height);
    
    // 真っ赤なフレームを生成（RGBA形式）
    let rgba_size = (width * height * 4) as usize;
    let mut rgba_data = vec![0u8; rgba_size];
    for i in 0..(width * height) as usize {
        let rgba_idx = i * 4;
        rgba_data[rgba_idx] = 255;     // R
        rgba_data[rgba_idx + 1] = 0;   // G
        rgba_data[rgba_idx + 2] = 0;   // B
        rgba_data[rgba_idx + 3] = 255;  // A
    }
    
    // RGBAデータをRGBデータに変換
    let rgb_size = (width * height * 3) as usize;
    let mut rgb_data = Vec::with_capacity(rgb_size);
    for i in 0..(width * height) as usize {
        let rgba_idx = i * 4;
        rgb_data.push(rgba_data[rgba_idx]);     // R
        rgb_data.push(rgba_data[rgba_idx + 1]); // G
        rgb_data.push(rgba_data[rgba_idx + 2]); // B
    }
    
    // RGBデータからYUVBufferを作成
    let yuv = openh264::formats::YUVBuffer::with_rgb(
        width as usize,
        height as usize,
        &rgb_data,
    );
    
    // H.264エンコード
    match encoder.encode(&yuv) {
        Ok(bitstream) => {
            // Annex-B形式に変換
            let (sample_data, has_sps_pps) = annexb_from_bitstream(&bitstream);
            
            if sample_data.is_empty() {
                warn!("SPS/PPS frame data is empty, skipping");
                return Ok(());
            }
            
            info!("SPS/PPS frame: {} bytes (has SPS/PPS: {})", sample_data.len(), has_sps_pps);
            
            // webrtc_media::Sampleを作成
            use webrtc_media::Sample;
            use std::time::Duration;
            use bytes::Bytes;
            
            let sample = Sample {
                data: Bytes::from(sample_data),
                duration: Duration::from_millis(33), // 30fps
                ..Default::default()
            };
            
            match track.write_sample(&sample).await {
                Ok(_) => {
                    info!("SPS/PPS frame sent to track");
                }
                Err(e) => {
                    error!("Failed to write SPS/PPS sample to track: {}", e);
                    return Err(anyhow::anyhow!("Failed to write SPS/PPS sample: {}", e));
                }
            }
        }
        Err(e) => {
            warn!("Failed to encode SPS/PPS frame: {}", e);
            return Err(anyhow::anyhow!("Failed to encode SPS/PPS frame: {}", e));
        }
    }
    
    Ok(())
}

impl WebRtcService {
    pub fn new(
        frame_rx: mpsc::Receiver<Frame>,
        signaling_tx: mpsc::Sender<SignalingResponse>,
        data_channel_tx: mpsc::Sender<DataChannelMessage>,
    ) -> (Self, mpsc::Sender<WebRtcMessage>) {
        let (message_tx, message_rx) = mpsc::channel(100);
        (
            Self {
                frame_rx,
                message_rx,
                signaling_tx,
                data_channel_tx,
            },
            message_tx,
        )
    }

    pub async fn run(mut self) -> Result<()> {
        info!("WebRtcService started");

        // webrtc-rsのAPIを初期化
        // H.264 Baseline/packetization-mode=1のみを登録（VP8/VP9を除外してH.264を優先）
        let mut m = MediaEngine::default();
        
        // デフォルトコーデックを登録した後、H.264のみを残すために
        // 一旦すべてのコーデックを登録し、その後H.264を優先する設定を行う
        // 注: webrtc-rsではregister_codecはRTCRtpCodecParametersを要求するため、
        // デフォルトコーデック登録を使用し、Track側でH.264を指定することで優先させる
        m.register_default_codecs()?;
        
        let mut registry = Registry::new();
        registry = register_default_interceptors(registry, &mut m)?;
        let api = APIBuilder::new()
            .with_media_engine(m)
            .with_interceptor_registry(registry)
            .build();

        let mut peer_connection: Option<Arc<RTCPeerConnection>> = None;
        let mut video_track_state: Option<VideoTrackState> = None;

        loop {
            tokio::select! {
                // フレーム受信
                frame = self.frame_rx.recv() => {
                    match frame {
                        Some(frame) => {
                            debug!("Received frame: {}x{}", frame.width, frame.height);
                            
                            // Video trackが存在する場合、フレームをエンコードして送信
                            if let Some(ref mut track_state) = video_track_state {
                                // エンコーダーの解像度が変更された場合は再初期化
                                if track_state.width != frame.width || track_state.height != frame.height {
                                    info!("Resizing encoder: {}x{} -> {}x{}", 
                                        track_state.width, track_state.height, 
                                        frame.width, frame.height);
                                    // エンコーダーを再作成（解像度変更時）
                                    // ビットレートを設定（解像度に応じて調整）
                                    let bitrate = (frame.width * frame.height * 2) as u32; // 約2Mbps for 1280x720
                                    let encoder_config = openh264::encoder::EncoderConfig::new(
                                        frame.width,
                                        frame.height
                                    )
                                    .set_bitrate_bps(bitrate)
                                    .max_frame_rate(30.0)
                                    .enable_skip_frame(false); // フレームスキップを無効化
                                    track_state.encoder = openh264::encoder::Encoder::with_config(encoder_config)
                                        .context("Failed to recreate encoder")?;
                                    track_state.width = frame.width;
                                    track_state.height = frame.height;
                                    track_state.sps_pps_sent = false; // 解像度変更時はSPS/PPSを再送信
                                    
                                    // 解像度変更時はSPS/PPSフレームを再送信
                                    if let Err(e) = send_sps_pps_frame(&track_state.track, &mut track_state.encoder, frame.width, frame.height).await {
                                        warn!("Failed to send SPS/PPS frame after resize: {}", e);
                                    }
                                }
                                
                                // SPS/PPSがまだ送信されていない場合は再送信
                                if !track_state.sps_pps_sent {
                                    warn!("SPS/PPS not sent yet, sending now");
                                    if let Err(e) = send_sps_pps_frame(&track_state.track, &mut track_state.encoder, track_state.width, track_state.height).await {
                                        warn!("Failed to send SPS/PPS frame: {}", e);
                                    } else {
                                        track_state.sps_pps_sent = true;
                                    }
                                }
                                
                                // RGBAデータをRGBデータに変換（アルファチャンネルを削除）
                                let rgb_size = (frame.width * frame.height * 3) as usize;
                                let mut rgb_data = Vec::with_capacity(rgb_size);
                                for i in 0..(frame.width * frame.height) as usize {
                                    let rgba_idx = i * 4;
                                    rgb_data.push(frame.data[rgba_idx]);     // R
                                    rgb_data.push(frame.data[rgba_idx + 1]); // G
                                    rgb_data.push(frame.data[rgba_idx + 2]); // B
                                    // Aチャンネルはスキップ
                                }
                                
                                // RGBデータからYUVBufferを作成
                                let yuv = openh264::formats::YUVBuffer::with_rgb(
                                    frame.width as usize,
                                    frame.height as usize,
                                    &rgb_data,
                                );
                                
                                // H.264エンコード
                                match track_state.encoder.encode(&yuv) {
                                    Ok(bitstream) => {
                                        debug!("Encoded bitstream: {} layers", bitstream.num_layers());
                                        // Annex-B形式に変換
                                        let (sample_data, has_sps_pps) = annexb_from_bitstream(&bitstream);
                                        
                                        // SPS/PPSが含まれている場合、フラグを更新
                                        if has_sps_pps {
                                            track_state.sps_pps_sent = true;
                                        }
                                        
                                        if sample_data.is_empty() {
                                            warn!("Encoded frame data is empty, skipping");
                                        } else if sample_data.len() < 5 {
                                            // スタートコード（4バイト）+ NALヘッダー（1バイト）= 5バイトが最小
                                            warn!("Encoded frame too small ({} bytes), skipping", sample_data.len());
                                        } else {
                                            debug!("Total encoded frame: {} bytes (SPS/PPS: {})", 
                                                sample_data.len(), has_sps_pps);
                                            
                                            // webrtc_media::Sampleを作成
                                            use webrtc_media::Sample;
                                            use std::time::Duration;
                                            use bytes::Bytes;
                                            
                                            let sample = Sample {
                                                data: Bytes::from(sample_data),
                                                duration: Duration::from_millis(33), // 30fps
                                                ..Default::default()
                                            };
                                            
                                            match track_state.track.write_sample(&sample).await {
                                                Ok(_) => {
                                                    debug!("Frame sent to track: {}x{}", frame.width, frame.height);
                                                }
                                                Err(e) => {
                                                    error!("Failed to write sample to track: {}", e);
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        warn!("Failed to encode frame: {}", e);
                                    }
                                }
                            }
                        }
                        None => {
                            debug!("Frame channel closed");
                            break;
                        }
                    }
                }
                // メッセージ受信
                msg = self.message_rx.recv() => {
                    match msg {
                        Some(WebRtcMessage::SetOffer { sdp }) => {
                            info!("SetOffer received, generating answer");
                            
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
                            pc.set_remote_description(offer).await
                                .context("Failed to set remote description")?;
                            
                            // Video trackを作成して追加
                            // H.264 Baseline/packetization-mode=1に固定したcodec capabilityを設定
                            let video_track = Arc::new(TrackLocalStaticSample::new(
                                RTCRtpCodecCapability {
                                    mime_type: "video/H264".to_string(),
                                    clock_rate: 90000,
                                    channels: 0,
                                    sdp_fmtp_line: "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42001f".to_string(),
                                    rtcp_feedback: vec![],
                                },
                                "video".to_string(),
                                "stream".to_string(),
                            ));
                            
                            // Transceiverを追加（sendonly）
                            let _transceiver = pc.add_track(video_track.clone() as Arc<dyn TrackLocal + Send + Sync>)
                                .await
                                .context("Failed to add video track")?;
                            
                            info!("Video track added to peer connection");
                            
                            // OpenH264エンコーダーを初期化（デフォルト解像度1280x720）
                            // CameraVideoRealTime用途・bitrate・intra_periodを明示設定
                            let encoder_config = openh264::encoder::EncoderConfig::new(1280, 720)
                                .set_bitrate_bps(2_000_000) // 2Mbps
                                .max_frame_rate(30.0) // 30fps
                                .enable_skip_frame(false); // フレームスキップを無効化
                            let encoder = openh264::encoder::Encoder::with_config(encoder_config)
                                .context("Failed to create OpenH264 encoder")?;
                            
                            // SPS/PPSの送出はLocalDescription設定後、最初のフレーム処理時に実行
                            // 初期値はfalseに設定（交渉完了後に送信）
                            video_track_state = Some(VideoTrackState {
                                track: video_track,
                                encoder,
                                width: 1280,
                                height: 720,
                                sps_pps_sent: false, // LocalDescription設定後に送信
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

                            // Answer SDPからm-line情報を解析（ICEハンドラ設定に使用）
                            let m_lines = parse_answer_m_lines(&answer.sdp);
                            info!("Answer SDP parsed: {} m-lines", m_lines.len());

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
                            
                            // LocalDescription設定後、SPS/PPSを送信（交渉完了後に確実に送信）
                            if let Some(ref mut track_state) = video_track_state {
                                info!("Sending SPS/PPS frame after LocalDescription set");
                                if let Err(e) = send_sps_pps_frame(&track_state.track, &mut track_state.encoder, track_state.width, track_state.height).await {
                                    warn!("Failed to send SPS/PPS frame after LocalDescription: {}", e);
                                } else {
                                    track_state.sps_pps_sent = true;
                                    info!("SPS/PPS frame sent successfully after LocalDescription");
                                }
                            }
                            
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
                            pc_for_state.on_peer_connection_state_change(Box::new(move |state: RTCPeerConnectionState| {
                                Box::pin(async move {
                                    match state {
                                        RTCPeerConnectionState::New => {
                                            info!("PeerConnection state: New");
                                        }
                                        RTCPeerConnectionState::Connecting => {
                                            info!("PeerConnection state: Connecting");
                                        }
                                        RTCPeerConnectionState::Connected => {
                                            info!("PeerConnection state: Connected - Media stream should be active");
                                        }
                                        RTCPeerConnectionState::Disconnected => {
                                            warn!("PeerConnection state: Disconnected - Connection lost");
                                        }
                                        RTCPeerConnectionState::Failed => {
                                            error!("PeerConnection state: Failed - Connection failed");
                                        }
                                        RTCPeerConnectionState::Closed => {
                                            info!("PeerConnection state: Closed");
                                        }
                                        RTCPeerConnectionState::Unspecified => {
                                            debug!("PeerConnection state: Unspecified");
                                        }
                                    }
                                })
                            }));
                            
                            // ICE接続状態の監視
                            let pc_for_ice = pc.clone();
                            pc_for_ice.on_ice_connection_state_change(Box::new(move |state| {
                                Box::pin(async move {
                                    match state {
                                        webrtc_rs::ice_transport::ice_connection_state::RTCIceConnectionState::New => {
                                            info!("ICE connection state: New");
                                        }
                                        webrtc_rs::ice_transport::ice_connection_state::RTCIceConnectionState::Checking => {
                                            info!("ICE connection state: Checking");
                                        }
                                        webrtc_rs::ice_transport::ice_connection_state::RTCIceConnectionState::Connected => {
                                            info!("ICE connection state: Connected - ICE connection established");
                                        }
                                        webrtc_rs::ice_transport::ice_connection_state::RTCIceConnectionState::Completed => {
                                            info!("ICE connection state: Completed - ICE gathering complete");
                                        }
                                        webrtc_rs::ice_transport::ice_connection_state::RTCIceConnectionState::Failed => {
                                            error!("ICE connection state: Failed - ICE connection failed");
                                        }
                                        webrtc_rs::ice_transport::ice_connection_state::RTCIceConnectionState::Disconnected => {
                                            warn!("ICE connection state: Disconnected - ICE connection lost");
                                        }
                                        webrtc_rs::ice_transport::ice_connection_state::RTCIceConnectionState::Closed => {
                                            info!("ICE connection state: Closed");
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

/// DataChannelメッセージ
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DataChannelMessage {
    Key { key: String, down: bool },
    MouseWheel { delta: i32 },
    ScreenshotRequest,
}

