use anyhow::Result;
use tokio::sync::mpsc;
use tracing::{debug, info};

use core_types::DataChannelMessage;

/// 入力サービス
pub struct InputService {
    message_rx: mpsc::Receiver<DataChannelMessage>,
}

impl InputService {
    pub fn new(message_rx: mpsc::Receiver<DataChannelMessage>) -> Self {
        Self { message_rx }
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
                // 後で実装
            }
        }
        Ok(())
    }
}

