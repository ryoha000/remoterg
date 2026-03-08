mod analyze;
mod keyboard_mouse;
mod llm;

use anyhow::Result;
use core_types::{CaptureMessage, DataChannelMessage, OutgoingDataChannelMessage};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, info};

use character_identifier::CharacterIdentifier;
use tagger::TaggerService;
use title_resolver::{TitleResolveResult, TitleResolver};
use vndb_client::{VndbCharacter, VndbClient};
use window_info::WindowInfoProvider;

/// キャラクター情報のキャッシュ
pub(crate) struct CharacterCache {
    pub(crate) vndb_id: String,
    pub(crate) _characters: Vec<VndbCharacter>,
}

pub(crate) const PROMPT_BASE: &str = r#"以下のJSONスキーマに従って、スクリーンショットの解析結果を出力してください。
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

pub(crate) const MAX_CHARACTER_IMAGES: usize = 32;
pub(crate) const MAX_CHARACTERS: usize = 50;

/// 入力サービス
pub struct InputService {
    pub(crate) message_rx: mpsc::Receiver<DataChannelMessage>,
    pub(crate) capture_cmd_tx: mpsc::Sender<CaptureMessage>,
    pub(crate) outgoing_dc_tx: mpsc::Sender<OutgoingDataChannelMessage>,
    pub(crate) tagger_service: TaggerService,
    pub(crate) tagger_cmd_tx: mpsc::Sender<core_types::TaggerCommand>,
    pub(crate) screenshot_dir: PathBuf,
    pub(crate) characters_dir: PathBuf,
    pub(crate) target_hwnd: u64,
    pub(crate) window_info_provider: WindowInfoProvider,
    pub(crate) title_resolver: Option<Arc<TitleResolver>>,
    pub(crate) cached_title: tokio::sync::Mutex<Option<(String, TitleResolveResult)>>,
    pub(crate) vndb_client: VndbClient,
    pub(crate) cached_characters: tokio::sync::Mutex<Option<CharacterCache>>,
    pub(crate) character_identifier: Option<Arc<tokio::sync::Mutex<CharacterIdentifier>>>,
}

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
            }
            DataChannelMessage::Pong { timestamp: _ } => {}
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
}
