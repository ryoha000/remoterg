use crate::InputService;
use anyhow::Result;
use core_types::{DataChannelMessage, OutgoingDataChannelMessage};
use tracing::error;

impl InputService {
    pub(crate) async fn handle_get_llm_config(&self) -> Result<()> {
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

    pub(crate) async fn handle_update_llm_config(
        &self,
        config: core_types::LlmConfig,
    ) -> Result<()> {
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
