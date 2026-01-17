use anyhow::Result;
use image::ColorType;
use image::ImageEncoder;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info};
use uuid::Uuid;

use core_types::{
    CaptureMessage, DataChannelMessage, Frame, OutgoingDataChannelMessage, ScreenshotMetadataPayload,
};

/// 入力サービス
pub struct InputService {
    message_rx: mpsc::Receiver<DataChannelMessage>,
    capture_cmd_tx: mpsc::Sender<CaptureMessage>,
    outgoing_dc_tx: mpsc::Sender<OutgoingDataChannelMessage>,
}

impl InputService {
    pub fn new(
        message_rx: mpsc::Receiver<DataChannelMessage>,
        capture_cmd_tx: mpsc::Sender<CaptureMessage>,
        outgoing_dc_tx: mpsc::Sender<OutgoingDataChannelMessage>,
    ) -> Self {
        Self {
            message_rx,
            capture_cmd_tx,
            outgoing_dc_tx,
        }
    }

    pub async fn run(mut self) -> Result<()> {
        info!("InputService started");

        loop {
            match self.message_rx.recv().await {
                Some(msg) => {
                    debug!("Received input message: {:?}", msg);
                    self.handle_message(msg).await?;
                }
                None => {
                    debug!("Input message channel closed");
                    break;
                }
            }
        }

        info!("InputService stopped");
        Ok(())
    }

    async fn handle_message(&self, msg: DataChannelMessage) -> Result<()> {
        match msg {
            DataChannelMessage::Key { key, down } => {
                info!("Key input: {} (down: {})", key, down);
                // 後でWin32 SendInputを実装
            }
            DataChannelMessage::MouseWheel { delta } => {
                info!("Mouse wheel: {}", delta);
                // 後でWin32 SendInputを実装
            }
            DataChannelMessage::ScreenshotRequest => {
                info!("Screenshot requested");
                self.handle_screenshot_request().await?;
            }
            DataChannelMessage::Ping { timestamp } => {
                debug!("Ping received: timestamp={}", timestamp);
                // Pingメッセージは接続の生存確認用なので、特に処理は不要
            }
            DataChannelMessage::Pong { timestamp: _ } => {
                // Pong receives are ignored
            }
            _ => {
                debug!("Unhandled message: {:?}", msg);
            }
        }
        Ok(())
    }

    async fn handle_screenshot_request(&self) -> Result<()> {
        // 1. Request frame from CaptureService
        let (tx, rx) = oneshot::channel::<Frame>();
        self.capture_cmd_tx
            .send(CaptureMessage::RequestFrame { tx })
            .await?;

        // Wait for frame (with timeout)
        let frame = match tokio::time::timeout(tokio::time::Duration::from_millis(500), rx).await {
            Ok(Ok(frame)) => frame,
            Ok(Err(e)) => {
                error!("Failed to receive frame from CaptureService: {}", e);
                return Ok(());
            }
            Err(_) => {
                error!("Timeout waiting for frame from CaptureService");
                return Ok(());
            }
        };

        // 2. Encode to PNG
        // The frame data is BGRA (Windows Capture default)
        // Convert BGRA to RGBA if needed, or just tell the encoder strictly.
        // image crate supports Bgra8 so we can use that if available, or just swap.
        // But let's check `image` crate features. Usually `ColorType::Rgba8` expects R,G,B,A.
        // Windows Desktop Duplication usually returns BGRA.
        // `Frame` struct in `core` has raw bytes.
        // Let's assume we need to swap B and R.
        let width = frame.width;
        let height = frame.height;
        let mut rgba_data = frame.data;

        // BGRA -> RGBA conversion
        for chunk in rgba_data.chunks_exact_mut(4) {
             let b = chunk[0];
             let r = chunk[2];
             chunk[0] = r;
             chunk[2] = b;
        }

        let mut png_data = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new(&mut png_data);
        encoder.write_image(&rgba_data, width, height, ColorType::Rgba8.into())?;

        // 3. Create Metadata
        let id = Uuid::new_v4().to_string();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let total_size = png_data.len() as u32;

        let metadata = DataChannelMessage::ScreenshotMetadata {
            payload: ScreenshotMetadataPayload {
                id: id.clone(),
                timestamp,
                format: "png".to_string(),
                width,
                height,
                size: total_size,
            },
        };

        // 4. Send Metadata
        self.outgoing_dc_tx
            .send(OutgoingDataChannelMessage::Text(metadata))
            .await?;

        // 5. Send Binary Chunks
        // Chunk size 16KB (WebRTC safe limit is usually higher like 64KB or 256KB, but 16KB is safe)
        const CHUNK_SIZE: usize = 16 * 1024;
        let total_chunks = (png_data.len() + CHUNK_SIZE - 1) / CHUNK_SIZE;

        for chunk in png_data.chunks(CHUNK_SIZE) {
            // We just send raw binary for now as per spec "Meta -> Binary Transfer".
            // If the client expects just the image stream, we send chunks.
            // But if we need ordering or ID, we might need a header.
            // "受信側は ordered: true により順序通りに受信し、メタデータのIDと紐付けて結合する。"
            // This implies the binary stream is PURELY the image data for that ID.
            // Since `ordered: true`, we can just blast the bytes.
            // But wait, what if other messages interleave?
            // "メタデータ送信直後に画像のバイナリデータを送信する"
            // If we send other control messages in between, the client might get confused if it blindly concatenates binary messages.
            // But DataChannelMessage is JSON (text).
            // Binary messages are distinct type.
            // If we only send screenshot data as binary, then all binary messages are screenshot chunks.
            self.outgoing_dc_tx
                .send(OutgoingDataChannelMessage::Binary(chunk.to_vec()))
                .await?;
        }

        info!("Sent screenshot {} ({} bytes, {} chunks)", id, png_data.len(), total_chunks);

        Ok(())
    }
}
