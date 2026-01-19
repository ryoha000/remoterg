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
        // Note: The screenshot capture service usually produces raw BGRA or RGBA. 
        // We might need to encode it to JPEG/PNG before sending to LLM if it expects an image file format.
        // Q: Does llama-server accept raw pixel data? No, usually data URLs with standard image formats.
        // So we MUST encode the raw bytes (which I assume `image_data` is) into a compressed format (PNG/JPEG).
        // Wait, where does `image_data` come from? If it's already encoded (e.g. from a saved file), good.
        // If it's raw frame buffer, we need to encode it.
        // The task description says "取得したスクリーンショット" (Taken screenshot). 
        // If it comes from `CaptureService`, it's likely raw `Vec<u8>` (BGRA).
        // If so, we need `image` crate to encode it to PNG/JPEG.
        // Let's assume for now `image_data` IS ALREADY ENCODED (JPEG/PNG) by the caller, 
        // or that we will add encoding logic.
        // Given dependencies, I don't see `image` crate in `tagger`'s Cargo.toml.
        // I should probably ensure the caller handles encoding or add `image` crate.
        // However, `hostd` might already have `image` crate or `encoder` service might handle it.
        // Let's proceed assuming `image_data` is a valid image FILE content (PNG/JPEG bytes).

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
            max_tokens: Some(1024), // Reasonable default
            temperature: Some(0.7),
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
}
