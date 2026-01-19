use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::{Child, Command};
use tracing::{debug, info, warn};

pub struct TaggerSetup {
    child: Option<Child>,
}

impl TaggerSetup {
    pub fn new() -> Self {
        Self { child: None }
    }

    pub async fn start(&mut self, port: u16, server_path: Option<PathBuf>) -> Result<()> {
        let server_path = self.resolve_server_path(server_path)?;
        let model_path = self.resolve_model_path(&server_path)?;
        let mmproj_path = self.resolve_mmproj_path(&server_path)?;
        let use_gpu = self.check_gpu_availability().await;

        let mut args = vec![
            "-m".to_string(),
            model_path.to_string_lossy().to_string(),
            "--mmproj".to_string(),
            mmproj_path.to_string_lossy().to_string(),
            "--port".to_string(),
            port.to_string(),
            "-fa".to_string(), // Flash Attention
            "on".to_string(),
        ];

        if use_gpu {
            info!("NVIDIA GPU detected, enabling GPU offload");
            args.push("--n-gpu-layers".to_string());
            args.push("999".to_string());
        } else {
            warn!("NVIDIA GPU not detected, falling back to CPU/Software");
        }

        let exe_path = server_path.join("llama-server.exe");
        if !exe_path.exists() {
            warn!("llama-server.exe not found at {:?}. LLM features will be unavailable.", exe_path);
            return Ok(());
        }

        info!("Starting llama-server: {:?} {:?}", exe_path, args);

        let child = Command::new(exe_path)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .context("Failed to spawn llama-server")?;

        self.child = Some(child);
        info!("llama-server started on port {}", port);

        Ok(())
    }

    fn resolve_server_path(&self, server_path: Option<PathBuf>) -> Result<PathBuf> {
        if let Some(path) = server_path {
             debug!("Using provided server path: {:?}", path);
             return Ok(path);
        }

        // Fallback to executable directory
        let current_exe = std::env::current_exe()?;
        let parent = current_exe
            .parent()
            .context("Failed to get parent directory of current executable")?;
        
        debug!("Using fallback path (adjacent to executable): {:?}", parent);
        Ok(parent.to_path_buf())
    }

    fn resolve_model_path(&self, server_path: &Path) -> Result<PathBuf> {
        let model_path = server_path.join("Qwen3VL-8B-Instruct-Q4_K_M.gguf");
        // We don't strictly error if missing here, just return the path to let llama-server fail or we handle it later.
        // But better to check.
        if !model_path.exists() {
             warn!("Model file not found at {:?}. llama-server might fail.", model_path);
        }
        Ok(model_path)
    }

    fn resolve_mmproj_path(&self, server_path: &Path) -> Result<PathBuf> {
        let model_path = server_path.join("mmproj-Qwen3VL-8B-Instruct-F16.gguf");
        // We don't strictly error if missing here, just return the path to let llama-server fail or we handle it later.
        // But better to check.
        if !model_path.exists() {
             warn!("mmproj file not found at {:?}. llama-server might fail.", model_path);
        }
        Ok(model_path)
    }

    async fn check_gpu_availability(&self) -> bool {
        // Simple check using nvidia-smi
        match Command::new("nvidia-smi")
            .arg("--query-gpu=name")
            .arg("--format=csv,noheader")
            .output()
            .await 
        {
            Ok(output) => {
                if output.status.success() {
                    let gpu_name = String::from_utf8_lossy(&output.stdout);
                    debug!("NVIDIA GPU detected: {}", gpu_name.trim());
                    true
                } else {
                    debug!("nvidia-smi failed with status: {}", output.status);
                    false
                }
            }
            Err(e) => {
                debug!("Failed to execute nvidia-smi: {}", e);
                false
            }
        }
    }

    pub async fn shutdown(&mut self) -> Result<()> {
        if let Some(mut child) = self.child.take() {
            info!("Stopping llama-server...");
            child.kill().await?;
            child.wait().await?;
            info!("llama-server stopped");
        }
        Ok(())
    }
}
