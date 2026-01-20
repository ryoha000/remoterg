use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::{Child, Command};
use tracing::{debug, info, warn};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE};

pub struct TaggerSetup {
    child: Option<Child>,
    job_handle: Option<HANDLE>,
    current_port: u16,
    current_server_path: Option<PathBuf>,
    current_model_path: Option<PathBuf>,
    current_mmproj_path: Option<PathBuf>,
}


impl TaggerSetup {
    pub fn new() -> Self {
        Self {
            child: None,
            job_handle: None,
            current_port: 8081,
            current_server_path: None,
            current_model_path: None,
            current_mmproj_path: None,
        }
    }

    pub async fn start(
        &mut self,
        port: u16,
        server_path: Option<PathBuf>,
        custom_model_path: Option<PathBuf>,
        custom_mmproj_path: Option<PathBuf>,
    ) -> Result<()> {
        self.current_port = port;
        self.current_server_path = server_path.clone();
        self.current_model_path = custom_model_path.clone();
        self.current_mmproj_path = custom_mmproj_path.clone();

        let server_path = self.resolve_server_path(server_path)?;
        let model_path = if let Some(p) = custom_model_path {
            if !p.exists() {
                warn!("Custom model file not found at {:?}. llama-server might fail.", p);
            }
            p
        } else {
            self.resolve_model_path(&server_path)?
        };
        self.current_model_path = Some(model_path.clone());

        let mmproj_path = if let Some(p) = custom_mmproj_path {
             if !p.exists() {
                warn!("Custom mmproj file not found at {:?}. llama-server might fail.", p);
            }
            p
        } else {
             self.resolve_mmproj_path(&server_path)?
        };
        self.current_mmproj_path = Some(mmproj_path.clone());

        // Ensure Job Object is created
        if self.job_handle.is_none() {
            unsafe {
                let job = CreateJobObjectW(None, None)?;
                let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const _,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )?;
                self.job_handle = Some(job);
            }
        }

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

        if let Some(job) = self.job_handle {
            if let Some(pid) = child.id() {
                let process_handle_res = unsafe {
                    OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, false, pid)
                };
                match process_handle_res {
                    Ok(process_handle) => {
                        unsafe {
                            if let Err(e) = AssignProcessToJobObject(job, process_handle) {
                                warn!("Failed to assign llama-server to Job Object: {}", e);
                            }
                            let _ = CloseHandle(process_handle);
                        }
                    }
                    Err(e) => {
                         warn!("Failed to open process handle for llama-server (PID: {}): {}", pid, e);
                    }
                }
            }
        }

        self.child = Some(child);
        info!("llama-server started on port {}", port);

        Ok(())
    }

    pub async fn restart(
        &mut self,
        port: u16,
        server_path: Option<PathBuf>,
        custom_model_path: Option<PathBuf>,
        custom_mmproj_path: Option<PathBuf>,
    ) -> Result<()> {
        self.shutdown().await?;
        self.start(port, server_path, custom_model_path, custom_mmproj_path).await
    }

    pub fn get_config(&self) -> (u16, Option<PathBuf>, Option<PathBuf>) {
        (self.current_port, self.current_model_path.clone(), self.current_mmproj_path.clone())
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
        if let Some(job) = self.job_handle.take() {
            unsafe { let _ = CloseHandle(job); }
        }
        Ok(())
    }
}
