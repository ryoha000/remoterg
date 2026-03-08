use crate::{CharacterCache, InputService, MAX_CHARACTERS, MAX_CHARACTER_IMAGES, PROMPT_BASE};
use anyhow::Result;
use core_types::{
    AnalysisCharacter, CaptureMessage, DataChannelMessage, OutgoingDataChannelMessage,
    ScreenshotFrame, ScreenshotMetadataPayload,
};
use image::ColorType;
use image::ImageEncoder;
use tokio::sync::oneshot;
use tracing::{error, info};
use uuid::Uuid;

pub enum ImageSource {
    Memory(Vec<u8>),
    File(String),
}

impl InputService {
    pub(crate) async fn handle_screenshot_request(&self, include_image: bool, max_edge: u32) -> Result<()> {
        // 1. Request screenshot from CaptureService
        let (tx, rx) = oneshot::channel::<ScreenshotFrame>();
        self.capture_cmd_tx
            .send(CaptureMessage::GetScreenshot { tx })
            .await?;

        // Wait for screenshot (with timeout)
        let screenshot =
            match tokio::time::timeout(tokio::time::Duration::from_millis(500), rx).await {
                Ok(Ok(screenshot)) => screenshot,
                Ok(Err(e)) => {
                    error!("Failed to receive screenshot from CaptureService: {}", e);
                    return Ok(());
                }
                Err(_) => {
                    error!("Timeout waiting for screenshot from CaptureService");
                    return Ok(());
                }
            };

        // 2. Encode to JPEG for performance
        let width = screenshot.width;
        let height = screenshot.height;

        let mut jpeg_data = Vec::new();
        // Use JPEG with quality 80
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_data, 80);
        encoder.write_image(&screenshot.data, width, height, ColorType::Rgba8.into())?;

        // 3. Create Metadata
        let id = Uuid::new_v4().to_string();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let total_size = jpeg_data.len() as u32;

        // --- Save to Server ---
        if !self.screenshot_dir.exists() {
            tokio::fs::create_dir_all(&self.screenshot_dir).await?;
        }

        let file_path = self.screenshot_dir.join(format!("{}.jpeg", id));
        tokio::fs::write(&file_path, &jpeg_data).await?;
        info!("Saved screenshot to: {:?}", file_path);
        // ----------------------

        // Create Window Metadata
        let window_info = match self.window_info_provider.get_info(self.target_hwnd) {
            Ok(info) => Some(info),
            Err(e) => {
                error!(
                    "Failed to get window info for hwnd {}: {}",
                    self.target_hwnd, e
                );
                None
            }
        };

        let process_path = window_info.as_ref().map(|i| i.process_path.clone());
        let mut title_info = None;

        if let Some(ref path) = process_path {
            if let Some(resolver) = &self.title_resolver {
                let mut cache = self.cached_title.lock().await;
                if let Some((ref cached_path, ref cached_result)) = *cache {
                    if cached_path == path {
                        title_info = Some(cached_result.clone());
                    }
                }

                if title_info.is_none() {
                    if let Some(result) = resolver.resolve(path) {
                        *cache = Some((path.clone(), result.clone()));
                        title_info = Some(result);
                    }
                }
            }
        }

        let metadata = DataChannelMessage::ScreenshotMetadata {
            payload: ScreenshotMetadataPayload {
                id: id.clone(),
                timestamp,
                format: "jpeg".to_string(),
                width,
                height,
                size: if include_image { total_size } else { 0 },
                window_title: window_info.as_ref().map(|i| i.title.clone()),
                process_path: window_info.as_ref().map(|i| i.process_path.clone()),
                process_name: window_info.as_ref().map(|i| i.process_name.clone()),
                vndb_id: title_info.as_ref().map(|t| t.vndb_id.clone()),
                official_title: title_info.as_ref().map(|t| t.official_title.clone()),
            },
        };

        // 4. Send Metadata
        self.outgoing_dc_tx
            .send(OutgoingDataChannelMessage::Text(metadata))
            .await?;

        // 5. Send Binary Chunks if requested
        if include_image {
            const CHUNK_SIZE: usize = 16 * 1024;
            let total_chunks = (jpeg_data.len() + CHUNK_SIZE - 1) / CHUNK_SIZE;

            for chunk in jpeg_data.chunks(CHUNK_SIZE) {
                self.outgoing_dc_tx
                    .send(OutgoingDataChannelMessage::Binary(chunk.to_vec()))
                    .await?;
            }

            info!(
                "Sent screenshot {} ({} bytes, {} chunks)",
                id,
                jpeg_data.len(),
                total_chunks
            );
        } else {
            info!("Sent screenshot metadata only for {}", id);
        }

        // Prepare and spawn AI analysis task
        self.prepare_and_spawn_analysis(
            id,
            ImageSource::Memory(jpeg_data),
            max_edge,
            title_info,
        ).await;

        Ok(())
    }

    pub(crate) async fn handle_analyze_request(&self, id: String, max_edge: u32) -> Result<()> {
        info!("Starting explicit AI analysis for {}", id);
        self.prepare_and_spawn_analysis(
            id.clone(),
            ImageSource::File(id),
            max_edge,
            None,
        ).await;

        Ok(())
    }

    pub(crate) async fn prepare_and_spawn_analysis(
        &self,
        id: String,
        source: ImageSource,
        max_edge: u32,
        title_info: Option<title_resolver::TitleResolveResult>,
    ) {
        // 1. Get image data
        let image_data = match source {
            ImageSource::Memory(data) => data,
            ImageSource::File(file_id) => {
                let file_path = self.screenshot_dir.join(format!("{}.jpeg", file_id));
                if !file_path.exists() {
                    error!("Requested analysis for missing screenshot: {}", file_id);
                    return;
                }
                match tokio::fs::read(&file_path).await {
                    Ok(data) => {
                        info!("Read screenshot file: {:?} ({} bytes)", file_path, data.len());
                        data
                    }
                    Err(e) => {
                        error!("Failed to read screenshot file {}: {}", file_id, e);
                        return;
                    }
                }
            }
        };

        // 2. Character Recognition (VNDB + ONNX)
        if let Some(ref title) = title_info {
            let mut char_cache = self.cached_characters.lock().await;
            let need_fetch = match &*char_cache {
                Some(cache) => cache.vndb_id != title.vndb_id,
                None => true,
            };

            if need_fetch {
                info!("VNDB キャラクター取得開始: {}", title.vndb_id);
                match self
                    .vndb_client
                    .get_characters(&title.vndb_id, MAX_CHARACTERS)
                    .await
                {
                    Ok(characters) => {
                        info!("VNDB キャラクター取得成功: {}件", characters.len());
                        // 画像ダウンロード
                        let images = self
                            .vndb_client
                            .download_character_images(
                                &characters,
                                MAX_CHARACTER_IMAGES,
                                &self.characters_dir,
                                &title.vndb_id,
                            )
                            .await;
                        info!("キャラ画像ダウンロード完了: {}枚", images.len());

                        if let Some(ci_arc) = &self.character_identifier {
                            let mut ci = ci_arc.lock().await;
                            if let Err(e) = ci
                                .register_references(
                                    &images,
                                    &self.characters_dir.join(&title.vndb_id),
                                    &title.vndb_id,
                                )
                                .await
                            {
                                error!("Failed to register character references: {}", e);
                            } else {
                                info!("Character references registered successfully.");
                            }
                        }

                        *char_cache = Some(CharacterCache {
                            vndb_id: title.vndb_id.clone(),
                            _characters: characters,
                        });
                    }
                    Err(e) => {
                        error!("VNDB キャラクター取得失敗: {}", e);
                    }
                }
            }
        }

        let chars_in_screenshot = if let Some(ci_arc) = &self.character_identifier {
            let mut ci = ci_arc.lock().await;
            match ci.identify(&image_data) {
                Ok(results) => results,
                Err(e) => {
                    error!("Character identification failed: {}", e);
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };

        let total_detected = chars_in_screenshot.len();

        let identified_known: Vec<_> = chars_in_screenshot
            .into_iter()
            .filter(|c| c.name != "Unknown")
            .collect();

        let total_identified = identified_known.len();
        info!(
            "Character Identifier: Detected {} characters, successfully identified {}",
            total_detected, total_identified
        );

        let analysis_characters: Vec<AnalysisCharacter> = identified_known
            .into_iter()
            .map(|c| {
                let position_str = match c.position_index {
                    0 => "Left",
                    1 => "Center",
                    2 => "Right",
                    _ => "Unknown",
                }
                .to_string();

                AnalysisCharacter {
                    name: c.name.clone(),
                    position: position_str,
                }
            })
            .collect();

        // 3. Spawn LLM background task
        let tagger_service = self.tagger_service.clone();
        let outgoing_tx = self.outgoing_dc_tx.clone();
        let prompt = PROMPT_BASE.to_string();

        tokio::spawn(async move {
            info!("Starting background AI analysis for {} with max_edge {}", id, max_edge);
            Self::execute_image_analysis(
                tagger_service,
                outgoing_tx,
                id.clone(),
                image_data,
                max_edge,
                &prompt,
                analysis_characters,
            )
            .await;
            info!("Finished background AI analysis for {}", id);
        });
    }

    pub(crate) async fn execute_image_analysis(
        tagger_service: tagger::TaggerService,
        outgoing_dc_tx: tokio::sync::mpsc::Sender<OutgoingDataChannelMessage>,
        id: String,
        image_data: Vec<u8>,
        max_edge: u32,
        prompt: &str,
        analysis_characters: Vec<AnalysisCharacter>,
    ) {
        // 1. Resize if needed
        let image_data_for_analysis = match image::load_from_memory(&image_data) {
            Ok(img) => {
                let width = img.width();
                let height = img.height();

                if width > max_edge || height > max_edge {
                    info!(
                        "Resizing image for analysis from {}x{} to max_edge {}",
                        width, height, max_edge
                    );
                    let resized =
                        img.resize(max_edge, max_edge, image::imageops::FilterType::Lanczos3);

                    let mut resized_data = Vec::new();
                    let mut cursor = std::io::Cursor::new(&mut resized_data);

                    match resized.write_to(&mut cursor, image::ImageOutputFormat::Png) {
                        Ok(_) => resized_data,
                        Err(e) => {
                            error!("Failed to encode resized image: {}", e);
                            image_data // fallback to original
                        }
                    }
                } else {
                    image_data
                }
            }
            Err(e) => {
                error!("Failed to load image for resizing: {}", e);
                image_data // fallback
            }
        };

        // 2. Call Tagger
        let result = match tagger_service
            .analyze_screenshot(&image_data_for_analysis, prompt)
            .await
        {
            Ok(text) => text,
            Err(e) => {
                error!("Tagger analysis failed: {}", e);
                // Send Error response
                let fallback_analysis = core_types::AnalysisResultPayload {
                    dialogue: Some(core_types::AnalysisDialoguePayload {
                        speaker: "System".to_string(),
                        text: format!("Error: {}", e),
                    }),
                    characters: Some(analysis_characters),
                };
                let response = DataChannelMessage::AnalyzeResponse {
                    id: id.clone(),
                    analysis: Some(fallback_analysis),
                };
                let _ = outgoing_dc_tx
                    .send(OutgoingDataChannelMessage::Text(response))
                    .await;
                return;
            }
        };

        info!("LLM AI Analysis Result for {}:\n{}", id, result);

        let parsed_analysis = extract_and_parse_json(&result);
        let final_analysis = match parsed_analysis {
            Some(mut a) => {
                a.characters = Some(analysis_characters);
                Some(a)
            }
            None => Some(core_types::AnalysisResultPayload {
                dialogue: None,
                characters: Some(analysis_characters),
            }),
        };

        // 3. Send Single Message containing everything
        let response = DataChannelMessage::AnalyzeResponse {
            id: id.clone(),
            analysis: final_analysis,
        };
        let _ = outgoing_dc_tx
            .send(OutgoingDataChannelMessage::Text(response))
            .await;
    }
}

fn extract_and_parse_json(text: &str) -> Option<core_types::AnalysisResultPayload> {
    if let (Some(start), Some(end)) = (text.find('{'), text.rfind('}')) {
        if start <= end {
            let json_slice = &text[start..=end];
            match serde_json::from_str::<core_types::AnalysisResultPayload>(json_slice) {
                Ok(parsed) => return Some(parsed),
                Err(e) => {
                    error!("Failed to parse extracted JSON: {}. Extracted: {}", e, json_slice);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_and_parse_json_pure() {
        let text = r#"{"dialogue":{"speaker":"兎亜","text":"女の子としてなんて……本気で言ってるの？"}}"#;
        let parsed = extract_and_parse_json(text).unwrap();
        assert_eq!(parsed.dialogue.unwrap().speaker, "兎亜");
    }

    #[test]
    fn test_extract_and_parse_json_markdown() {
        let text = r#"Here is the output:
```json
{
  "dialogue": {
    "speaker": "Alice",
    "text": "Hello, world!"
  }
}
```"#;
        let parsed = extract_and_parse_json(text).unwrap();
        assert_eq!(parsed.dialogue.unwrap().speaker, "Alice");
    }

    #[test]
    fn test_extract_and_parse_json_garbage() {
        let text = r#"Some thoughts:
The speaker appears to be Bob.
{
  "dialogue": {
    "speaker": "Bob",
    "text": "Wait a minute."
  }
}
And that's all.
"#;
        let parsed = extract_and_parse_json(text).unwrap();
        assert_eq!(parsed.dialogue.unwrap().speaker, "Bob");
    }
}

