use anyhow::{Context, Result};
use core_types::{SignalingResponse, VideoCodec, WebRtcMessage};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};
use tracing::{debug, error, info, warn};
use url::Url;

/// シグナリングメッセージ（Cloudflare経由で送受信）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SignalingMessage {
    #[serde(rename = "offer")]
    Offer {
        sdp: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        codec: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        negotiation_id: Option<String>,
    },
    #[serde(rename = "answer")]
    Answer {
        sdp: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        negotiation_id: Option<String>,
    },
    #[serde(rename = "error")]
    Error { message: String },
    #[serde(rename = "ice_candidate")]
    IceCandidate {
        candidate: String,
        sdp_mid: Option<String>,
        sdp_mline_index: Option<u16>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        negotiation_id: Option<String>,
    },
}

/// シグナリングクライアント（WebSocketクライアント）
pub struct SignalingClient {
    cloudflare_url: String,
    session_id: String,
    webrtc_tx: mpsc::Sender<WebRtcMessage>,
    signaling_rx: mpsc::Receiver<SignalingResponse>,
}

impl SignalingClient {
    pub fn new(
        cloudflare_url: String,
        session_id: String,
        webrtc_tx: mpsc::Sender<WebRtcMessage>,
        signaling_rx: mpsc::Receiver<SignalingResponse>,
    ) -> Self {
        Self {
            cloudflare_url,
            session_id,
            webrtc_tx,
            signaling_rx,
        }
    }

    pub async fn run(mut self) -> Result<()> {
        info!(
            "Starting SignalingClient connecting to {} (session_id: {})",
            self.cloudflare_url, self.session_id
        );

        let mut retry_count = 0;
        const MAX_RETRIES: u32 = 10;
        const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
        const MAX_BACKOFF: Duration = Duration::from_secs(60);

        let cloudflare_url = self.cloudflare_url.clone();
        let session_id = self.session_id.clone();
        let webrtc_tx = self.webrtc_tx.clone();
        // ReceiverはCloneできないため、Arc<Mutex<Receiver>>にラップ
        let signaling_rx = Arc::new(tokio::sync::Mutex::new(self.signaling_rx));

        loop {
            match Self::connect_and_run(
                cloudflare_url.clone(),
                session_id.clone(),
                webrtc_tx.clone(),
                signaling_rx.clone(),
            )
            .await
            {
                Ok(()) => {
                    info!("SignalingClient connection closed normally");
                    break;
                }
                Err(e) => {
                    error!("SignalingClient error: {}", e);
                    retry_count += 1;

                    if retry_count >= MAX_RETRIES {
                        error!("Max retries reached, giving up");
                        return Err(e);
                    }

                    // Exponential backoff
                    let backoff = INITIAL_BACKOFF
                        .mul_f64(2_f64.powi(retry_count as i32 - 1))
                        .min(MAX_BACKOFF);
                    warn!(
                        "Retrying in {:?} (attempt {}/{})",
                        backoff, retry_count, MAX_RETRIES
                    );
                    sleep(backoff).await;
                }
            }
        }

        Ok(())
    }

    async fn connect_and_run(
        cloudflare_url: String,
        session_id: String,
        webrtc_tx: mpsc::Sender<WebRtcMessage>,
        signaling_rx: Arc<tokio::sync::Mutex<mpsc::Receiver<SignalingResponse>>>,
    ) -> Result<()> {
        // WebSocket URLを構築
        let mut url = Url::parse(&cloudflare_url).context("Failed to parse cloudflare_url")?;
        url.query_pairs_mut()
            .append_pair("session_id", &session_id)
            .append_pair("role", "host");

        info!("Connecting to WebSocket: {}", url);

        // WebSocket接続
        let (ws_stream, _) = connect_async(url.as_str())
            .await
            .context("Failed to connect to WebSocket")?;

        info!("WebSocket connected");

        let (mut write, mut read) = ws_stream.split();

        // WebRTCサービスからの応答をWebSocketに送信するタスク
        let signaling_rx_for_write = signaling_rx.clone();
        let session_id_clone = session_id.clone();
        let mut write_handle = tokio::spawn(async move {
            loop {
                let response = {
                    let mut rx = signaling_rx_for_write.lock().await;
                    rx.recv().await
                };

                let Some(response) = response else {
                    break;
                };
                let message = match response {
                    SignalingResponse::Answer { sdp } => SignalingMessage::Answer {
                        sdp,
                        session_id: Some(session_id_clone.clone()),
                        negotiation_id: Some("default".to_string()),
                    },
                    SignalingResponse::IceCandidate {
                        candidate,
                        sdp_mid,
                        sdp_mline_index,
                    } => SignalingMessage::IceCandidate {
                        candidate,
                        sdp_mid,
                        sdp_mline_index,
                        session_id: Some(session_id_clone.clone()),
                        negotiation_id: Some("default".to_string()),
                    },
                    SignalingResponse::Error { message } => SignalingMessage::Error { message },
                };

                if let Ok(json) = serde_json::to_string(&message) {
                    if let Err(e) = write.send(WsMessage::Text(json.into())).await {
                        error!("Failed to send message to WebSocket: {}", e);
                        break;
                    }
                }
            }
            Ok::<(), anyhow::Error>(())
        });

        // WebSocketからのメッセージを受信してWebRTCサービスに転送するタスク
        let webrtc_tx_recv = webrtc_tx.clone();
        let recv_handle = tokio::spawn(async move {
            while let Some(msg) = read.next().await {
                match msg {
                    Ok(WsMessage::Text(text)) => {
                        debug!("Received message: {}", text);
                        match serde_json::from_str::<SignalingMessage>(&text) {
                            Ok(SignalingMessage::Offer { sdp, codec, .. }) => {
                                let parsed_codec = parse_codec_param(codec);
                                info!("Offer received from signaling server, forwarding to WebRTC service (codec: {:?})", parsed_codec);
                                if let Err(e) = webrtc_tx_recv
                                    .send(WebRtcMessage::SetOffer {
                                        sdp,
                                        codec: parsed_codec,
                                    })
                                    .await
                                {
                                    error!("Failed to send offer to WebRTC service: {}", e);
                                    break;
                                }
                                info!("Offer forwarded to WebRTC service successfully");
                            }
                            Ok(SignalingMessage::IceCandidate {
                                candidate,
                                sdp_mid,
                                sdp_mline_index,
                                ..
                            }) => {
                                debug!("ICE candidate received, forwarding to WebRTC service");
                                if let Err(e) = webrtc_tx_recv
                                    .send(WebRtcMessage::AddIceCandidate {
                                        candidate,
                                        sdp_mid,
                                        sdp_mline_index,
                                    })
                                    .await
                                {
                                    error!("Failed to send ICE candidate to WebRTC service: {}", e);
                                }
                            }
                            Ok(SignalingMessage::Error { message }) => {
                                error!("Received error from signaling server: {}", message);
                            }
                            Ok(SignalingMessage::Answer { .. }) => {
                                warn!("Received Answer message as host (unexpected)");
                            }
                            Err(e) => {
                                error!("Failed to parse message: {}", e);
                            }
                        }
                    }
                    Ok(WsMessage::Close(_)) => {
                        info!("WebSocket closed by server");
                        break;
                    }
                    Ok(_) => {
                        debug!("Received non-text message");
                    }
                    Err(e) => {
                        error!("WebSocket error: {}", e);
                        break;
                    }
                }
            }
            Ok::<(), anyhow::Error>(())
        });

        // どちらかのタスクが終了するまで待機
        tokio::select! {
            result = &mut write_handle => {
                if let Err(e) = result {
                    error!("Write task error: {}", e);
                }
            }
            result = recv_handle => {
                if let Err(e) = result {
                    error!("Receive task error: {}", e);
                }
            }
        }

        Ok(())
    }
}

fn parse_codec_param(codec: Option<String>) -> Option<VideoCodec> {
    if let Some(codec_str) = codec {
        if codec_str.eq_ignore_ascii_case("any") || codec_str.is_empty() {
            return None;
        }
        codec_str.parse::<VideoCodec>().ok()
    } else {
        None
    }
}
