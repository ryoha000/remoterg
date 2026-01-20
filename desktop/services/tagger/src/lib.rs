use anyhow::{Context, Result};
use base64::prelude::*;
use reqwest::Client;
use serde::{Deserialize, Serialize};

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
                        image_url: ImageUrl {
                            url: data_url,
                        },
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

                                if let Ok(chunk) = serde_json::from_str::<ChatCompletionChunk>(data) {
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
}
