use anyhow::{Context, Result};
use base64::prelude::*;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::info;

#[derive(Clone)]
pub struct TaggerService {
    client: Client,
    base_url: String,
}

#[derive(Serialize)]
struct ChatCompletionRequest {
    messages: Vec<Message>,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Serialize)]
struct Message {
    role: String,
    content: Vec<ContentPart>,
}

#[derive(Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
enum ContentPart {
    Text { text: String },
    ImageUrl { image_url: ImageUrl },
}

#[derive(Serialize)]
struct ImageUrl {
    url: String,
}

#[derive(Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Deserialize)]
struct ResponseMessage {
    content: Option<String>,
}

#[derive(Deserialize)]
struct ChatCompletionChunk {
    choices: Vec<ChunkChoice>,
}

#[derive(Deserialize)]
struct ChunkChoice {
    delta: ChunkDelta,
}

#[derive(Deserialize)]
struct ChunkDelta {
    content: Option<String>,
}

/// 画像データのマジックバイトから MIME タイプを判定
fn detect_image_mime(data: &[u8]) -> &'static str {
    if data.len() >= 2 && data[0] == 0xFF && data[1] == 0xD8 {
        "image/jpeg"
    } else if data.len() >= 4 && &data[..4] == b"\x89PNG" {
        "image/png"
    } else if data.len() >= 4 && &data[..4] == b"RIFF" {
        "image/webp"
    } else {
        // フォールバック: JPEG として扱う
        "image/jpeg"
    }
}

impl TaggerService {
    pub fn new(port: u16) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .unwrap_or_else(|_| Client::new()),
            base_url: format!("http://127.0.0.1:{}", port),
        }
    }

    pub async fn analyze_screenshot(&self, image_data: &[u8], prompt: &str) -> Result<String> {
        let base64_image = BASE64_STANDARD.encode(image_data);
        let data_url = format!("data:image/png;base64,{}", base64_image);

        let request = ChatCompletionRequest {
            messages: vec![Message {
                role: "user".to_string(),
                content: vec![
                    ContentPart::Text {
                        text: prompt.to_string(),
                    },
                    ContentPart::ImageUrl {
                        image_url: ImageUrl { url: data_url },
                    },
                ],
            }],
            max_tokens: Some(512),
            temperature: Some(0.7),
            stream: None,
        };

        let response = self
            .client
            .post(format!("{}/v1/chat/completions", self.base_url))
            .json(&request)
            .send()
            .await
            .context("Failed to send request to llama-server")?
            .error_for_status()
            .context("llama-server returned error status")?
            .json::<ChatCompletionResponse>()
            .await
            .context("Failed to parse response from llama-server")?;

        let content = response
            .choices
            .first()
            .context("No choices returned from llama-server")?
            .message
            .content
            .clone()
            .unwrap_or_default();

        Ok(content)
    }

    pub async fn analyze_screenshot_stream(
        &self,
        image_data: &[u8],
        prompt: &str,
    ) -> Result<tokio::sync::mpsc::Receiver<Result<String>>> {
        let base64_image = BASE64_STANDARD.encode(image_data);
        let data_url = format!("data:image/png;base64,{}", base64_image);

        let request = ChatCompletionRequest {
            messages: vec![Message {
                role: "user".to_string(),
                content: vec![
                    ContentPart::Text {
                        text: prompt.to_string(),
                    },
                    ContentPart::ImageUrl {
                        image_url: ImageUrl { url: data_url },
                    },
                ],
            }],
            max_tokens: Some(512),
            temperature: Some(0.7),
            stream: Some(true),
        };

        let client = self.client.clone();
        let url = format!("{}/v1/chat/completions", self.base_url);
        let (tx, rx) = tokio::sync::mpsc::channel(100);

        tokio::spawn(async move {
            let res = match client
                .post(url)
                .json(&request)
                .send()
                .await
                .context("Failed to send request")
            {
                Ok(res) => res,
                Err(e) => {
                    let _ = tx.send(Err(e)).await;
                    return;
                }
            };

            if let Err(e) = res.error_for_status_ref() {
                let _ = tx.send(Err(anyhow::anyhow!("Server error: {}", e))).await;
                return;
            }

            use futures::StreamExt;
            let mut stream = res.bytes_stream();
            let mut buffer = String::new();

            while let Some(item) = stream.next().await {
                match item {
                    Ok(bytes) => {
                        let chunk_str = String::from_utf8_lossy(&bytes);
                        buffer.push_str(&chunk_str);

                        while let Some(idx) = buffer.find('\n') {
                            let line = buffer[..idx].trim().to_string();
                            buffer = buffer[idx + 1..].to_string();

                            if line.starts_with("data: ") {
                                let data = &line[6..];
                                if data == "[DONE]" {
                                    return;
                                }

                                if let Ok(chunk) = serde_json::from_str::<ChatCompletionChunk>(data)
                                {
                                    if let Some(choice) = chunk.choices.first() {
                                        if let Some(content) = &choice.delta.content {
                                            if tx.send(Ok(content.clone())).await.is_err() {
                                                return; // Receiver dropped
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(anyhow::anyhow!("Stream error: {}", e))).await;
                        break;
                    }
                }
            }
        });

        Ok(rx)
    }

    /// キャラクター参考画像付きでスクリーンショットを分析（ストリーミング）
    ///
    /// `reference_images`: (キャラ名, 画像データ) のスライス。
    /// プロンプト → 参考画像群 → スクリーンショットの順で content に配置する。
    pub async fn analyze_with_references(
        &self,
        screenshot: &[u8],
        reference_images: &[(String, String, Vec<u8>)],
        prompt: &str,
    ) -> Result<tokio::sync::mpsc::Receiver<Result<String>>> {
        let mut content_parts: Vec<ContentPart> = Vec::new();

        // 1. テキストプロンプト
        content_parts.push(ContentPart::Text {
            text: prompt.to_string(),
        });

        let mut idx = 0;
        // 2. 参考キャラ画像
        for (name, description, image_data) in reference_images {
            if idx > 16 {
                break;
            }
            
            let mut text = format!("参考: キャラクター「{}」の立ち絵", name);
            if !description.is_empty() {
                text.push_str(&format!("\nキャラクター説明:\n{}", description));
            }
            
            content_parts.push(ContentPart::Text {
                text,
            });
            let mime_type = detect_image_mime(image_data);
            let base64 = BASE64_STANDARD.encode(image_data);
            content_parts.push(ContentPart::ImageUrl {
                image_url: ImageUrl {
                    url: format!("data:{};base64,{}", mime_type, base64),
                },
            });
            idx += 1;
        }

        // 3. 分析対象のスクリーンショット
        content_parts.push(ContentPart::Text {
            text: "以下が分析対象のスクリーンショットです:".to_string(),
        });
        let screenshot_mime = detect_image_mime(screenshot);
        let screenshot_base64 = BASE64_STANDARD.encode(screenshot);
        content_parts.push(ContentPart::ImageUrl {
            image_url: ImageUrl {
                url: format!("data:{};base64,{}", screenshot_mime, screenshot_base64),
            },
        });

        let request = ChatCompletionRequest {
            messages: vec![Message {
                role: "user".to_string(),   
                content: content_parts,
            }],
            max_tokens: Some(512),
            temperature: Some(0.7),
            stream: Some(true),
        };

        let client = self.client.clone();
        let url = format!("{}/v1/chat/completions", self.base_url);
        let (tx, rx) = tokio::sync::mpsc::channel(100);

        tokio::spawn(async move {
            let res = match client
                .post(url)
                .json(&request)
                .send()
                .await
                .context("Failed to send request")
            {
                Ok(res) => res,
                Err(e) => {
                    let _ = tx.send(Err(e)).await;
                    return;
                }
            };

            if let Err(e) = res.error_for_status_ref() {
                let _ = tx.send(Err(anyhow::anyhow!("Server error: {}", e))).await;
                return;
            }

            use futures::StreamExt;
            let mut stream = res.bytes_stream();
            let mut buffer = String::new();

            while let Some(item) = stream.next().await {
                match item {
                    Ok(bytes) => {
                        let chunk_str = String::from_utf8_lossy(&bytes);
                        buffer.push_str(&chunk_str);

                        while let Some(idx) = buffer.find('\n') {
                            let line = buffer[..idx].trim().to_string();
                            buffer = buffer[idx + 1..].to_string();

                            if line.starts_with("data: ") {
                                let data = &line[6..];
                                if data == "[DONE]" {
                                    return;
                                }

                                if let Ok(chunk) = serde_json::from_str::<ChatCompletionChunk>(data)
                                {
                                    if let Some(choice) = chunk.choices.first() {
                                        if let Some(content) = &choice.delta.content {
                                            if tx.send(Ok(content.clone())).await.is_err() {
                                                return;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(anyhow::anyhow!("Stream error: {}", e))).await;
                        break;
                    }
                }
            }
        });

        Ok(rx)
    }
}
