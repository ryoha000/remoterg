use anyhow::Result;
use axum::{
    body::Body,
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    http::Request,
    middleware::{self, Next},
    response::{Html, Response},
    routing::get,
    Router,
};
use core_types::{SignalingResponse, VideoCodec, WebRtcMessage};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, error, info, warn};

/// シグナリングメッセージ
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SignalingMessage {
    #[serde(rename = "offer")]
    Offer {
        sdp: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        codec: Option<String>,
    },
    #[serde(rename = "answer")]
    Answer { sdp: String },
    #[serde(rename = "error")]
    Error { message: String },
    #[serde(rename = "ice_candidate")]
    IceCandidate {
        candidate: String,
        sdp_mid: Option<String>,
        sdp_mline_index: Option<u16>,
    },
}

/// シグナリングサービスの状態
#[derive(Clone)]
struct SignalingState {
    webrtc_tx: mpsc::Sender<WebRtcMessage>,
    signaling_response_tx: broadcast::Sender<SignalingResponse>,
}

/// シグナリングサービス
pub struct SignalingService {
    port: u16,
    webrtc_tx: mpsc::Sender<WebRtcMessage>,
    signaling_rx: mpsc::Receiver<SignalingResponse>,
}

impl SignalingService {
    pub fn new(
        port: u16,
        webrtc_tx: mpsc::Sender<WebRtcMessage>,
        signaling_rx: mpsc::Receiver<SignalingResponse>,
    ) -> Self {
        Self {
            port,
            webrtc_tx,
            signaling_rx,
        }
    }

    pub async fn run(mut self) -> Result<()> {
        info!("Starting SignalingService on port {}", self.port);

        // グローバルな応答チャンネルを各接続で共有するため、broadcastチャンネルを使用
        let (signaling_response_tx, _) = broadcast::channel::<SignalingResponse>(100);

        // WebRTCサービスからの応答をbroadcastチャンネルに転送するタスク
        let global_tx = signaling_response_tx.clone();
        tokio::spawn(async move {
            while let Some(response) = self.signaling_rx.recv().await {
                if let Err(e) = global_tx.send(response) {
                    error!("Failed to forward signaling response: {}", e);
                    break;
                }
            }
        });

        let state = SignalingState {
            webrtc_tx: self.webrtc_tx.clone(),
            signaling_response_tx: signaling_response_tx.clone(),
        };

        let app = Router::new()
            .route("/", get(serve_index))
            .route("/signal", get(handle_websocket))
            .route("/ping", get(ping))
            // .nest_service("/static", ServeDir::new("static"))
            .with_state(state)
            // 全てのリクエストをログに残す
            .layer(middleware::from_fn(log_requests));

        // IPv6 デュアルスタックでバインドし、失敗した場合は IPv4 にフォールバック
        let v6_addr = format!("[::]:{}", self.port);
        let listener = match tokio::net::TcpListener::bind(&v6_addr).await {
            Ok(listener) => {
                info!(
                    "SignalingService listening on http://{} (dual stack)",
                    v6_addr
                );
                listener
            }
            Err(e) => {
                warn!("IPv6 dual-stack bind failed ({}), falling back to IPv4", e);
                let v4_addr = format!("0.0.0.0:{}", self.port);
                let listener = tokio::net::TcpListener::bind(&v4_addr).await?;
                info!("SignalingService listening on http://{}", v4_addr);
                listener
            }
        };

        axum::serve(listener, app).await?;
        info!("SignalingService stopped");
        Ok(())
    }
}

async fn serve_index() -> Html<&'static str> {
    Html(include_str!("../../web/index.html"))
}

async fn ping() -> &'static str {
    "pong"
}

async fn handle_websocket(ws: WebSocketUpgrade, State(state): State<SignalingState>) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn log_requests(req: Request<Body>, next: Next) -> Response {
    // リクエストメソッドとパスを記録
    let method = req.method().clone();
    let uri = req.uri().clone();
    info!("Incoming request: {} {}", method, uri);

    next.run(req).await
}

/// SDPのm-line情報
#[cfg(test)]
#[allow(dead_code)]
#[derive(Debug, Clone)]
struct MLine {
    media_type: String, // "video", "application" など
    mid: Option<String>,
    index: usize, // 元の順序
}

/// Offer SDPからm-lineの順序を解析
#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn parse_offer_m_lines(offer_sdp: &str) -> Vec<MLine> {
    let mut m_lines = Vec::new();
    let lines: Vec<&str> = offer_sdp.lines().collect();

    for (index, line) in lines.iter().enumerate() {
        if line.starts_with("m=") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if !parts.is_empty() {
                let media_type = parts[0].trim_start_matches("m=").to_string();

                // このm-lineのmidを探す（次のm=または終端まで）
                let mut mid = None;
                for next_line in lines.iter().skip(index + 1) {
                    if next_line.starts_with("m=") {
                        break; // 次のm-lineに到達
                    }
                    if next_line.starts_with("a=mid:") {
                        mid = Some(next_line.trim_start_matches("a=mid:").to_string());
                        break;
                    }
                }

                m_lines.push(MLine {
                    media_type,
                    mid,
                    index: m_lines.len(),
                });
            }
        }
    }

    m_lines
}

/// Offerに基づいてAnswer SDPを生成
#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn generate_answer_from_offer(offer_sdp: &str) -> String {
    let m_lines = parse_offer_m_lines(offer_sdp);
    debug!("Parsed {} m-lines from offer", m_lines.len());

    // Answerの基本ヘッダー
    let mut answer_lines = vec![
        "v=0".to_string(),
        "o=- 4611731400430051336 2 IN IP4 127.0.0.1".to_string(),
        "s=-".to_string(),
        "t=0 0".to_string(),
    ];

    // BUNDLEグループを生成（midのリスト）
    if !m_lines.is_empty() {
        let bundle_mids: Vec<String> = m_lines
            .iter()
            .enumerate()
            .map(|(i, _)| i.to_string())
            .collect();
        answer_lines.push(format!("a=group:BUNDLE {}", bundle_mids.join(" ")));
    }

    answer_lines.push("a=msid-semantic: WMS".to_string());

    // 各m-lineに対してAnswerを生成
    for (index, m_line) in m_lines.iter().enumerate() {
        match m_line.media_type.as_str() {
            "video" => {
                answer_lines.push("m=video 9 UDP/TLS/RTP/SAVPF 96".to_string());
                answer_lines.push("c=IN IP4 0.0.0.0".to_string());
                answer_lines.push("a=rtcp:9 IN IP4 0.0.0.0".to_string());
                answer_lines.push("a=ice-ufrag:testufrag".to_string());
                answer_lines.push("a=ice-pwd:testpwd123456789012345678".to_string());
                answer_lines.push("a=fingerprint:sha-256 AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA".to_string());
                answer_lines.push("a=setup:active".to_string());
                answer_lines.push(format!("a=mid:{}", index));
                answer_lines.push("a=sendrecv".to_string());
                answer_lines.push("a=rtcp-mux".to_string());
                answer_lines.push("a=rtpmap:96 H264/90000".to_string());
                answer_lines.push("a=fmtp:96 level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42001f".to_string());
            }
            "application" => {
                // DataChannel用のm-line
                answer_lines.push("m=application 9 UDP/DTLS/SCTP webrtc-datachannel".to_string());
                answer_lines.push("c=IN IP4 0.0.0.0".to_string());
                answer_lines.push("a=ice-ufrag:testufrag".to_string());
                answer_lines.push("a=ice-pwd:testpwd123456789012345678".to_string());
                answer_lines.push("a=fingerprint:sha-256 AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA".to_string());
                answer_lines.push("a=setup:active".to_string());
                answer_lines.push(format!("a=mid:{}", index));
                answer_lines.push("a=sctp-port:5000".to_string());
                answer_lines.push("a=max-message-size:262144".to_string());
            }
            _ => {
                // その他のメディアタイプはinactiveで応答
                answer_lines.push(format!("m={} 0 UDP/TLS/RTP/SAVPF", m_line.media_type));
                answer_lines.push("c=IN IP4 0.0.0.0".to_string());
                answer_lines.push(format!("a=mid:{}", index));
                answer_lines.push("a=inactive".to_string());
            }
        }
    }

    // CRLFで結合（WebRTCではCRLFが必要）
    answer_lines.join("\r\n") + "\r\n"
}

fn parse_codec_param(codec: Option<String>) -> Result<Option<VideoCodec>, String> {
    if let Some(codec_str) = codec {
        if codec_str.eq_ignore_ascii_case("any") || codec_str.is_empty() {
            return Ok(None);
        }
        codec_str
            .parse::<VideoCodec>()
            .map(Some)
            .map_err(|e| format!("無効なcodec指定: {}", e))
    } else {
        Ok(None)
    }
}

async fn handle_socket(mut socket: WebSocket, state: SignalingState) {
    info!("WebSocket client connected");

    // 簡易実装: グローバルなチャンネルを使用
    // TODO: 接続ごとのチャンネル管理を実装（複数接続対応）
    // 現時点では、WebRTCサービスがグローバルなチャンネルを使用するため、
    // 各接続は同じチャンネルを共有します（最初の接続のみが動作）
    let webrtc_tx = state.webrtc_tx.clone();

    // broadcastチャンネルからこの接続用の受信チャンネルを作成
    let mut signaling_response_rx = state.signaling_response_tx.subscribe();

    loop {
        tokio::select! {
            // WebSocketメッセージ受信
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        debug!("Received message: {}", text);

                        // JSONをパース
                        match serde_json::from_str::<SignalingMessage>(&text) {
                            Ok(SignalingMessage::Offer { sdp, codec }) => {
                                let parsed_codec = match parse_codec_param(codec) {
                                    Ok(c) => c,
                                    Err(msg) => {
                                        let error_msg = SignalingMessage::Error { message: msg };
                                        if let Ok(response_json) = serde_json::to_string(&error_msg) {
                                            let _ = socket.send(Message::Text(response_json)).await;
                                        }
                                        continue;
                                    }
                                };
                                debug!("Offer received, forwarding to WebRTC service");
                                // WebRTCサービスにOfferを転送
                                // 簡易実装: 応答チャンネルも一緒に送信する必要があるが、
                                // 現時点ではグローバルなチャンネルを使用
                                if let Err(e) = webrtc_tx
                                    .send(WebRtcMessage::SetOffer {
                                        sdp,
                                        codec: parsed_codec,
                                    })
                                    .await
                                {
                                    error!("Failed to send offer to WebRTC service: {}", e);
                                    break;
                                }
                                info!("Offer forwarded to WebRTC service");
                            }
                            Ok(SignalingMessage::IceCandidate { candidate, sdp_mid, sdp_mline_index }) => {
                                debug!("ICE candidate received, forwarding to WebRTC service");
                                // WebRTCサービスにICE candidateを転送
                                if let Err(e) = webrtc_tx.send(WebRtcMessage::AddIceCandidate {
                                    candidate,
                                    sdp_mid,
                                    sdp_mline_index,
                                }).await {
                                    error!("Failed to send ICE candidate to WebRTC service: {}", e);
                                }
                            }
                            Ok(_) => {
                                debug!("Received other signaling message");
                            }
                            Err(e) => {
                                error!("Failed to parse message: {}", e);
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) => {
                        info!("WebSocket closed by client");
                        break;
                    }
                    Some(Err(e)) => {
                        error!("WebSocket error: {}", e);
                        break;
                    }
                    None => {
                        debug!("WebSocket stream ended");
                        break;
                    }
                    _ => {}
                }
            }
            // WebRTCサービスからの応答受信（broadcastチャンネルから）
            result = signaling_response_rx.recv() => {
                match result {
                    Ok(response) => {
                        match response {
                            SignalingResponse::Answer { sdp } => {
                                info!("Answer received from WebRTC service, sending to client");
                                let answer = SignalingMessage::Answer { sdp };
                                match serde_json::to_string(&answer) {
                                    Ok(response_json) => {
                                        if let Err(e) = socket.send(Message::Text(response_json)).await {
                                            error!("Failed to send answer to client: {}", e);
                                            break;
                                        }
                                        info!("Answer sent to client");
                                    }
                                    Err(e) => {
                                        error!("Failed to serialize answer: {}", e);
                                    }
                                }
                            }
                            SignalingResponse::Error { message } => {
                                let err_msg = SignalingMessage::Error { message };
                                match serde_json::to_string(&err_msg) {
                                    Ok(response_json) => {
                                        if let Err(e) = socket.send(Message::Text(response_json)).await {
                                            error!("Failed to send error to client: {}", e);
                                            break;
                                        }
                                    }
                                    Err(e) => {
                                        error!("Failed to serialize error: {}", e);
                                    }
                                }
                            }
                            SignalingResponse::IceCandidate { candidate, sdp_mid, sdp_mline_index } => {
                                debug!("ICE candidate received from WebRTC service, sending to client");
                                let ice_candidate = SignalingMessage::IceCandidate {
                                    candidate,
                                    sdp_mid,
                                    sdp_mline_index,
                                };
                                match serde_json::to_string(&ice_candidate) {
                                    Ok(response_json) => {
                                        if let Err(e) = socket.send(Message::Text(response_json)).await {
                                            error!("Failed to send ICE candidate to client: {}", e);
                                        }
                                    }
                                    Err(e) => {
                                        error!("Failed to serialize ICE candidate: {}", e);
                                    }
                                }
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        warn!("Signaling response channel lagged, skipped {} messages", skipped);
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        debug!("Signaling response channel closed");
                        break;
                    }
                }
            }
        }
    }

    info!("WebSocket connection closed");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signaling_message_serialization() {
        let offer = SignalingMessage::Offer {
            sdp: "test sdp".to_string(),
            codec: Some("vp8".to_string()),
        };

        let json = serde_json::to_string(&offer).unwrap();
        assert!(json.contains("offer"));
        assert!(json.contains("test sdp"));

        let deserialized: SignalingMessage = serde_json::from_str(&json).unwrap();
        match deserialized {
            SignalingMessage::Offer { sdp, codec } => {
                assert_eq!(sdp, "test sdp");
                assert_eq!(codec, Some("vp8".to_string()));
            }
            _ => panic!("Expected Offer"),
        }
    }

    #[test]
    fn test_parse_offer_m_lines() {
        // videoとapplicationの両方を含むOffer
        let offer_sdp = r#"v=0
o=- 3039444489731279037 2 IN IP4 127.0.0.1
s=-
t=0 0
a=group:BUNDLE 0 1
a=msid-semantic: WMS
m=video 9 UDP/TLS/RTP/SAVPF 96
c=IN IP4 0.0.0.0
a=mid:0
a=sendrecv
m=application 9 UDP/DTLS/SCTP webrtc-datachannel
c=IN IP4 0.0.0.0
a=mid:1
"#;

        let m_lines = parse_offer_m_lines(offer_sdp);
        assert_eq!(m_lines.len(), 2);
        assert_eq!(m_lines[0].media_type, "video");
        assert_eq!(m_lines[0].mid, Some("0".to_string()));
        assert_eq!(m_lines[1].media_type, "application");
        assert_eq!(m_lines[1].mid, Some("1".to_string()));
    }

    #[test]
    fn test_generate_answer_preserves_order() {
        // applicationが先、videoが後の順序のOffer
        let offer_sdp = r#"v=0
o=- 3039444489731279037 2 IN IP4 127.0.0.1
s=-
t=0 0
a=group:BUNDLE 0 1
a=msid-semantic: WMS
m=application 9 UDP/DTLS/SCTP webrtc-datachannel
c=IN IP4 0.0.0.0
a=mid:0
m=video 9 UDP/TLS/RTP/SAVPF 96
c=IN IP4 0.0.0.0
a=mid:1
a=sendrecv
"#;

        let answer_sdp = generate_answer_from_offer(offer_sdp);

        // CRLFが使用されていることを確認
        assert!(answer_sdp.contains("\r\n"));

        // m-lineの順序が保持されていることを確認
        let lines: Vec<&str> = answer_sdp.lines().collect();
        let mut m_line_indices = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            if line.starts_with("m=") {
                m_line_indices.push(i);
            }
        }

        // applicationが先、videoが後であることを確認
        let application_line = lines[m_line_indices[0]];
        let video_line = lines[m_line_indices[1]];
        assert!(application_line.contains("application"));
        assert!(video_line.contains("video"));

        // midの順序も確認
        let mut mids = Vec::new();
        for line in &lines {
            if line.starts_with("a=mid:") {
                mids.push(line.trim_start_matches("a=mid:"));
            }
        }
        assert_eq!(mids[0], "0");
        assert_eq!(mids[1], "1");
    }

    #[test]
    fn test_generate_answer_crlf_format() {
        let offer_sdp = r#"v=0
o=- 3039444489731279037 2 IN IP4 127.0.0.1
s=-
t=0 0
m=video 9 UDP/TLS/RTP/SAVPF 96
a=mid:0
"#;

        let answer_sdp = generate_answer_from_offer(offer_sdp);

        // CRLFが使用されていることを確認（LFのみではない）
        assert!(answer_sdp.contains("\r\n"));
        // 最後がCRLFで終わっていることを確認
        assert!(answer_sdp.ends_with("\r\n"));
    }
}
