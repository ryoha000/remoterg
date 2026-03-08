use anyhow::Result;
use image::ColorType;
use image::ImageEncoder;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info};
use uuid::Uuid;

use tagger::TaggerService;

use core_types::{
    AnalysisCharacter, CaptureMessage, DataChannelMessage, OutgoingDataChannelMessage,
    ScreenshotFrame, ScreenshotMetadataPayload,
};

use character_identifier::CharacterIdentifier;
use std::sync::Arc;
use title_resolver::{TitleResolveResult, TitleResolver};
use vndb_client::{VndbCharacter, VndbClient};
use window_info::WindowInfoProvider;

use std::path::PathBuf;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_KEYUP,
    MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN,
    MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    MOUSEEVENTF_VIRTUALDESK, MOUSEINPUT, VK_LCONTROL, VK_LSHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, GetWindowRect, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
    SM_YVIRTUALSCREEN,
};

/// 入力サービス
pub struct InputService {
    message_rx: mpsc::Receiver<DataChannelMessage>,
    capture_cmd_tx: mpsc::Sender<CaptureMessage>,
    outgoing_dc_tx: mpsc::Sender<OutgoingDataChannelMessage>,
    tagger_service: TaggerService,
    tagger_cmd_tx: mpsc::Sender<core_types::TaggerCommand>,
    screenshot_dir: PathBuf,
    characters_dir: PathBuf,
    target_hwnd: u64,
    window_info_provider: WindowInfoProvider,
    title_resolver: Option<Arc<TitleResolver>>,
    cached_title: tokio::sync::Mutex<Option<(String, TitleResolveResult)>>,
    vndb_client: VndbClient,
    /// (vndb_id, キャラ一覧, ダウンロード済み画像)
    cached_characters: tokio::sync::Mutex<Option<CharacterCache>>,
    character_identifier: Option<Arc<tokio::sync::Mutex<CharacterIdentifier>>>,
}

/// キャラクター情報のキャッシュ
struct CharacterCache {
    vndb_id: String,
    _characters: Vec<VndbCharacter>,
}

const PROMPT_BASE: &str = r#"以下のJSONスキーマに従って、スクリーンショットの解析結果を出力してください。
解析できない項目がある場合は、nullまたは空配列を返してください。

### JSON Schema:
{
  "dialogue": {
    "speaker": "文字列: 名前欄に表示されている名前",
    "text": "文字列: メッセージウィンドウ内の全文（改行は \n で保持）"
  }
}

### 出力制約:
- JSON形式のみを出力し、それ以外の説明テキストは一切含めないでください。
"#;

/// キャラ画像付き分析に使用するキャラクターの最大数
const MAX_CHARACTER_IMAGES: usize = 32;
/// キャラ取得の最大件数
const MAX_CHARACTERS: usize = 50;

impl InputService {
    pub fn new(
        message_rx: mpsc::Receiver<DataChannelMessage>,
        capture_cmd_tx: mpsc::Sender<CaptureMessage>,
        outgoing_dc_tx: mpsc::Sender<OutgoingDataChannelMessage>,
        tagger_service: TaggerService,
        tagger_cmd_tx: mpsc::Sender<core_types::TaggerCommand>,
        screenshot_dir: PathBuf,
        characters_dir: PathBuf,
        target_hwnd: u64,
        title_resolver: Option<Arc<TitleResolver>>,
        character_identifier: Option<Arc<tokio::sync::Mutex<CharacterIdentifier>>>,
    ) -> Self {
        Self {
            message_rx,
            capture_cmd_tx,
            outgoing_dc_tx,
            tagger_service,
            tagger_cmd_tx,
            screenshot_dir,
            characters_dir,
            target_hwnd,
            window_info_provider: WindowInfoProvider::new(),
            title_resolver,
            cached_title: tokio::sync::Mutex::new(None),
            vndb_client: VndbClient::new(),
            cached_characters: tokio::sync::Mutex::new(None),
            character_identifier,
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
                self.handle_key_input(&key, down).await?;
            }
            DataChannelMessage::MouseWheel { delta } => {
                info!("Mouse wheel: {}", delta);
                // 後でWin32 SendInputを実装
            }
            DataChannelMessage::MouseClick { x, y, button } => {
                // info!("Mouse click: ({}, {}) button={}", x, y, button);
                self.handle_mouse_click(x, y, &button).await?;
            }
            DataChannelMessage::CursorMove { dx, dy } => {
                self.handle_cursor_move(dx, dy).await?;
            }
            DataChannelMessage::CursorClick { button } => {
                self.handle_cursor_click(&button).await?;
            }
            DataChannelMessage::ScreenshotRequest { include_image } => {
                info!("Screenshot requested (include_image: {})", include_image);
                self.handle_screenshot_request(include_image).await?;
            }
            DataChannelMessage::AnalyzeRequest { id, max_edge } => {
                info!(
                    "Analysis requested for screenshot: {} (max_edge: {})",
                    id, max_edge
                );
                self.handle_analyze_request(id, max_edge).await?;
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

    async fn handle_screenshot_request(&self, include_image: bool) -> Result<()> {
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
        // The screenshot data is RGBA
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
            // Chunk size 16KB (WebRTC safe limit is usually higher like 64KB or 256KB, but 16KB is safe)
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

        // --- キャラクター情報の取得（VNDB API） ---
        let (prompt, analysis_characters) = if let Some(ref title) = title_info {
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
                        // 画像ダウンロード（完了まで待機）
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

            match &*char_cache {
                Some(_cache) => {
                    let chars_in_screenshot = if let Some(ci_arc) = &self.character_identifier {
                        let mut ci = ci_arc.lock().await;
                        match ci.identify(&jpeg_data) {
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

                    (PROMPT_BASE.to_string(), analysis_characters)
                }
                None => (PROMPT_BASE.to_string(), Vec::new()),
            }
        } else {
            (PROMPT_BASE.to_string(), Vec::new())
        };

        // --- Auto AI Analysis Trigger ---
        let tagger_service = self.tagger_service.clone();
        let outgoing_tx = self.outgoing_dc_tx.clone();
        let image_data = jpeg_data;
        let id_for_task = id.clone();

        tokio::spawn(async move {
            info!("Starting background auto AI analysis for {}", id_for_task);

            // Resize for analysis
            let image_data_for_analysis = match image::load_from_memory(&image_data) {
                Ok(img) => {
                    let w = img.width();
                    let h = img.height();
                    let max_edge = 512;
                    if w > max_edge || h > max_edge {
                        let resized =
                            img.resize(max_edge, max_edge, image::imageops::FilterType::Lanczos3);
                        let mut resized_data = Vec::new();
                        let mut cursor = std::io::Cursor::new(&mut resized_data);
                        if let Ok(_) = resized.write_to(&mut cursor, image::ImageOutputFormat::Png)
                        {
                            resized_data
                        } else {
                            image_data
                        }
                    } else {
                        image_data
                    }
                }
                Err(_) => image_data,
            };

            let mut rx = match tagger_service
                .analyze_screenshot_stream(&image_data_for_analysis, &prompt)
                .await
            {
                Ok(rx) => rx,
                Err(e) => {
                    error!("Auto AI analysis failed for {}: {}", id_for_task, e);
                    return;
                }
            };

            while let Some(result) = rx.recv().await {
                match result {
                    Ok(delta) => {
                        let response = DataChannelMessage::AnalyzeResponseChunk {
                            id: id_for_task.clone(),
                            delta,
                        };
                        let _ = outgoing_tx
                            .send(OutgoingDataChannelMessage::Text(response))
                            .await;
                    }
                    Err(e) => {
                        error!("Stream error during auto AI analysis: {}", e);
                        break;
                    }
                }
            }

            let response = DataChannelMessage::AnalyzeResponseDone {
                id: id_for_task.clone(),
                characters: analysis_characters,
            };
            let _ = outgoing_tx
                .send(OutgoingDataChannelMessage::Text(response))
                .await;
            info!("Finished background auto AI analysis for {}", id_for_task);
        });

        Ok(())
    }

    async fn handle_analyze_request(&self, id: String, max_edge: u32) -> Result<()> {
        let file_path = self.screenshot_dir.join(format!("{}.jpeg", id));
        if !file_path.exists() {
            error!("Requested analysis for missing screenshot: {}", id);
            // Optionally send an error response back so client stops waiting
            return Ok(());
        }

        // 1. Read file
        let image_data = tokio::fs::read(&file_path).await?;
        info!(
            "Read screenshot file: {:?} ({} bytes)",
            file_path,
            image_data.len()
        );

        // 2. Resize if needed
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
                        Ok(_) => {
                            info!("Resized image size: {} bytes", resized_data.len());

                            // Save resized image
                            let resized_path =
                                self.screenshot_dir.join(format!("{}_resized.png", id));
                            if let Err(e) = tokio::fs::write(&resized_path, &resized_data).await {
                                error!("Failed to save resized image: {}", e);
                            } else {
                                info!("Saved resized image to: {:?}", resized_path);
                            }

                            resized_data
                        }
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

        // 3. Call Tagger
        let mut rx = match self
            .tagger_service
            .analyze_screenshot_stream(&image_data_for_analysis, PROMPT_BASE)
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
        let response = DataChannelMessage::AnalyzeResponseDone {
            id,
            characters: Vec::new(),
        };
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

    async fn handle_mouse_click(&self, x: f64, y: f64, button: &str) -> Result<()> {
        let (abs_x, abs_y) = if self.target_hwnd != 0 {
            let hwnd = HWND(self.target_hwnd as *mut _);
            let mut rect = windows::Win32::Foundation::RECT::default();
            unsafe {
                if GetWindowRect(hwnd, &mut rect).is_err() {
                    error!("Failed to get window rect for hwnd {}", self.target_hwnd);
                    return Ok(());
                }
            }
            let width = rect.right - rect.left;
            let height = rect.bottom - rect.top;

            let target_x = rect.left + (x * width as f64) as i32;
            let target_y = rect.top + (y * height as f64) as i32;

            self.map_to_virtual_screen(target_x, target_y)
        } else {
            // Full screen mapping (assuming primary monitor or simple scaling)
            // x, y are 0.0-1.0
            ((x * 65535.0) as i32, (y * 65535.0) as i32)
        };

        let (down_flag, up_flag) = match button.to_lowercase().as_str() {
            "right" => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
            "middle" => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
            _ => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
        };

        // Click sequence: Move -> Down -> Up
        // In SendInput, we can combine or just send separate events.
        // For reliability, Move then Click.

        let inputs = [
            INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: abs_x,
                        dy: abs_y,
                        mouseData: 0,
                        dwFlags: MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_MOVE | MOUSEEVENTF_VIRTUALDESK,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: abs_x,
                        dy: abs_y,
                        mouseData: 0,
                        dwFlags: MOUSEEVENTF_ABSOLUTE | down_flag | MOUSEEVENTF_VIRTUALDESK,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: abs_x,
                        dy: abs_y,
                        mouseData: 0,
                        dwFlags: MOUSEEVENTF_ABSOLUTE | up_flag | MOUSEEVENTF_VIRTUALDESK,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
        ];

        unsafe {
            SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        }

        Ok(())
    }

    async fn handle_cursor_move(&self, dx: i32, dy: i32) -> Result<()> {
        let inputs = [INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                mi: MOUSEINPUT {
                    dx,
                    dy,
                    mouseData: 0,
                    dwFlags: MOUSEEVENTF_MOVE,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }];

        unsafe {
            SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        }

        Ok(())
    }

    async fn handle_cursor_click(&self, button: &str) -> Result<()> {
        let (down_flag, up_flag) = match button.to_lowercase().as_str() {
            "right" => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
            "middle" => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
            _ => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
        };

        let inputs = [
            INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: 0,
                        dy: 0,
                        mouseData: 0,
                        dwFlags: down_flag,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: 0,
                        dy: 0,
                        mouseData: 0,
                        dwFlags: up_flag,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
        ];

        unsafe {
            SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        }

        Ok(())
    }

    async fn handle_key_input(&self, key: &str, down: bool) -> Result<()> {
        let vk_code = match key {
            "Control" => VK_LCONTROL,
            "Shift" => VK_LSHIFT,
            // 今後他のキーが必要になればここに追加
            _ => {
                debug!("Unsupported key: {}", key);
                return Ok(());
            }
        };

        // スキャンコードベースの入力より、仮想キーコードベースのシンプルな入力を行う
        let dw_flags = if down {
            windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS(0)
        } else {
            KEYEVENTF_KEYUP
        };

        let inputs = [INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk_code,
                    wScan: 0,
                    dwFlags: dw_flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }];

        unsafe {
            SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        }

        Ok(())
    }

    fn map_to_virtual_screen(&self, x: i32, y: i32) -> (i32, i32) {
        unsafe {
            let v_left = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let v_top = GetSystemMetrics(SM_YVIRTUALSCREEN);
            let v_width = GetSystemMetrics(SM_CXVIRTUALSCREEN);
            let v_height = GetSystemMetrics(SM_CYVIRTUALSCREEN);

            let abs_x = ((x - v_left) as f64 * 65535.0 / v_width as f64) as i32;
            let abs_y = ((y - v_top) as f64 * 65535.0 / v_height as f64) as i32;

            (abs_x, abs_y)
        }
    }
}
