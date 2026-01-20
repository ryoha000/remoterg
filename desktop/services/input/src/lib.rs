use anyhow::Result;
use image::ColorType;
use image::ImageEncoder;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info};
use uuid::Uuid;

use tagger::TaggerService;

use core_types::{
    CaptureMessage, DataChannelMessage, Frame, OutgoingDataChannelMessage, ScreenshotMetadataPayload,
};

use std::path::PathBuf;

/// 入力サービス
pub struct InputService {
    message_rx: mpsc::Receiver<DataChannelMessage>,
    capture_cmd_tx: mpsc::Sender<CaptureMessage>,
    outgoing_dc_tx: mpsc::Sender<OutgoingDataChannelMessage>,
    tagger_service: TaggerService,
    tagger_cmd_tx: mpsc::Sender<core_types::TaggerCommand>,
    screenshot_dir: PathBuf,
}

const PROMPT: &str = r#"以下のJSONスキーマに従って、スクリーンショットの解析結果を出力してください。
解析できない項目がある場合は、nullまたは空配列を返してください。

### JSON Schema:
{
  "scene_info": {
    "location": "文字列: 背景から推測される場所",
    "time_of_day": "文字列: 昼、夕方、夜、不明など",
    "atmosphere": "文字列: 場面の雰囲気（例：平穏、緊張、ロマンチック）"
  },
  "dialogue": {
    "speaker": "文字列: 名前欄に表示されている名前",
    "text": "文字列: メッセージウィンドウ内の全文（改行は \n で保持）"
  },
  "characters": [
    {
      "name": "文字列: キャラクター名",
      "expression_tags": ["文字列: 表情を示すタグ（例：微笑、怒り、照れ）"],
      "visual_description": "文字列: 服装やポーズの簡潔な説明",
      "position": "文字列: 画面内の位置（左、中央、右）"
    }
  ]
}

### 出力制約:
- JSON形式のみを出力し、それ以外の説明テキストは一切含めないでください。
"#;

impl InputService {
    pub fn new(
        message_rx: mpsc::Receiver<DataChannelMessage>,
        capture_cmd_tx: mpsc::Sender<CaptureMessage>,
        outgoing_dc_tx: mpsc::Sender<OutgoingDataChannelMessage>,
        tagger_service: TaggerService,
        tagger_cmd_tx: mpsc::Sender<core_types::TaggerCommand>,
        screenshot_dir: PathBuf,
    ) -> Self {
        Self {
            message_rx,
            capture_cmd_tx,
            outgoing_dc_tx,
            tagger_service,
            tagger_cmd_tx,
            screenshot_dir,
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
            DataChannelMessage::AnalyzeRequest { id } => {
                info!("Analysis requested for screenshot: {}", id);
                self.handle_analyze_request(id).await?;
            }
            DataChannelMessage::Ping { timestamp } => {
                debug!("Ping received: timestamp={}", timestamp);
                // Pingメッセージは接続の生存確認用なので、特に処理は不要
            }
            DataChannelMessage::Pong { timestamp: _ } => {
                // Pong receives are ignored
            }

            DataChannelMessage::GetLlmConfig => {
                info!("GetLlmConfig");
                self.handle_get_llm_config().await?;
            }
            DataChannelMessage::UpdateLlmConfig { config } => {
                info!("UpdateLlmConfig: {:?}", config);
                self.handle_update_llm_config(config).await?;
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

        let mut png_data = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new(&mut png_data);
        encoder.write_image(&frame.data, width, height, ColorType::Rgba8.into())?;

        // 3. Create Metadata
        let id = Uuid::new_v4().to_string();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let total_size = png_data.len() as u32;

        // --- Save to Server ---
        if !self.screenshot_dir.exists() {
            tokio::fs::create_dir_all(&self.screenshot_dir).await?;
        }

        let file_path = self.screenshot_dir.join(format!("{}.png", id));
        tokio::fs::write(&file_path, &png_data).await?;
        info!("Saved screenshot to: {:?}", file_path);
        // ----------------------

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

    async fn handle_analyze_request(&self, id: String) -> Result<()> {
        let file_path = self.screenshot_dir.join(format!("{}.png", id));
        if !file_path.exists() {
            error!("Requested analysis for missing screenshot: {}", id);
            // Optionally send an error response back so client stops waiting
            return Ok(());
        }

        // 1. Read file
        let image_data = tokio::fs::read(&file_path).await?;
        info!("Read screenshot file: {:?} ({} bytes)", file_path, image_data.len());

        // 2. Resize if needed
        let image_data_for_analysis = match image::load_from_memory(&image_data) {
            Ok(img) => {
                let width = img.width();
                let height = img.height();
                
                if width > 512 || height > 512 {
                    info!("Resizing image for analysis from {}x{}", width, height);
                    let resized = img.resize(512, 512, image::imageops::FilterType::Lanczos3);
                    
                    let mut resized_data = Vec::new();
                    let mut cursor = std::io::Cursor::new(&mut resized_data);
                    
                    match resized.write_to(&mut cursor, image::ImageOutputFormat::Png) {
                        Ok(_) => {
                            info!("Resized image size: {} bytes", resized_data.len());
                            
                            // Save resized image
                            let resized_path = self.screenshot_dir.join(format!("{}_resized.png", id));
                            if let Err(e) = tokio::fs::write(&resized_path, &resized_data).await {
                                error!("Failed to save resized image: {}", e);
                            } else {
                                info!("Saved resized image to: {:?}", resized_path);
                            }

                            resized_data
                        },
                        Err(e) => {
                            error!("Failed to encode resized image: {}", e);
                            image_data // fallback to original
                        }
                    }
                } else {
                    image_data
                }
            },
            Err(e) => {
                error!("Failed to load image for resizing: {}", e);
                image_data // fallback
            }
        };

        // 3. Call Tagger
        let mut rx = match self
            .tagger_service
            .analyze_screenshot_stream(&image_data_for_analysis, PROMPT)
            .await
        {
            Ok(rx) => rx,
            Err(e) => {
                error!("Tagger analysis failed: {}", e);
                let response = DataChannelMessage::AnalyzeResponse {
                    id: id.clone(),
                    text: format!("Error: {}", e),
                };
                self.outgoing_dc_tx
                    .send(OutgoingDataChannelMessage::Text(response))
                    .await?;
                return Ok(());
            }
        };

        info!("Analysis stream started for {}", id);

        while let Some(result) = rx.recv().await {
            match result {
                Ok(delta) => {
                    let response = DataChannelMessage::AnalyzeResponseChunk {
                        id: id.clone(),
                        delta,
                    };
                    self.outgoing_dc_tx
                        .send(OutgoingDataChannelMessage::Text(response))
                        .await?;
                }
                Err(e) => {
                    error!("Stream error during analysis: {}", e);
                    break;
                }
            }
        }

        // 4. Send Done
        let response = DataChannelMessage::AnalyzeResponseDone { id };
        self.outgoing_dc_tx
            .send(OutgoingDataChannelMessage::Text(response))
            .await?;

        info!("Sent analysis completion");
        Ok(())
    }

    async fn handle_get_llm_config(&self) -> Result<()> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        if let Err(e) = self
            .tagger_cmd_tx
            .send(core_types::TaggerCommand::GetConfig { reply_tx: tx })
            .await
        {
            error!("Failed to send GetConfig to hostd: {}", e);
            return Ok(());
        }

        match rx.await {
            Ok(config) => {
                let response = DataChannelMessage::LlmConfigResponse { config };
                self.outgoing_dc_tx
                    .send(OutgoingDataChannelMessage::Text(response))
                    .await?;
            }
            Err(e) => {
                error!("Failed to receive LlmConfig response: {}", e);
            }
        }
        Ok(())
    }

    async fn handle_update_llm_config(&self, config: core_types::LlmConfig) -> Result<()> {
        if let Err(e) = self
            .tagger_cmd_tx
            .send(core_types::TaggerCommand::UpdateConfig {
                config: config.clone(),
            })
            .await
        {
            error!("Failed to send UpdateConfig to hostd: {}", e);
            return Ok(());
        }

        Ok(())
    }
}
