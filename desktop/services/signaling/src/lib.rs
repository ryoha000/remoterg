// SignalingClient: WebSocketクライアントとしてCloudflareに接続
pub mod client;
pub use client::{SignalingClient, SignalingMessage};

// テスト用のユーティリティ関数
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    #[test]
    fn test_signaling_message_serialization() {
        let offer = SignalingMessage::Offer {
            sdp: "test sdp".to_string(),
            codec: Some("h264".to_string()),
            session_id: Some("test_session".to_string()),
            negotiation_id: Some("test_negotiation".to_string()),
        };

        let json = serde_json::to_string(&offer).unwrap();
        assert!(json.contains("offer"));
        assert!(json.contains("test sdp"));

        let deserialized: SignalingMessage = serde_json::from_str(&json).unwrap();
        match deserialized {
            SignalingMessage::Offer { sdp, codec, .. } => {
                assert_eq!(sdp, "test sdp");
                assert_eq!(codec, Some("h264".to_string()));
            }
            _ => panic!("Expected Offer"),
        }
    }
}
