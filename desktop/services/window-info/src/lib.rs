use anyhow::Result;
use windows::Win32::Foundation::{HANDLE, HWND, MAX_PATH};
use windows::Win32::System::ProcessStatus::GetModuleFileNameExW;
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
};

pub struct WindowInfoProvider;

#[derive(Debug, Clone)]
pub struct WindowMetadata {
    pub title: String,
    pub process_path: String,
    pub process_name: String,
}

impl WindowInfoProvider {
    pub fn new() -> Self {
        Self
    }

    pub fn get_info(&self, hwnd: u64) -> Result<WindowMetadata> {
        let hwnd = HWND(hwnd as *mut _);

        let title = self
            .get_window_title(hwnd)
            .unwrap_or_else(|_| "Unknown".to_string());
        let (process_path, process_name) = self
            .get_process_info(hwnd)
            .unwrap_or_else(|_| ("Unknown".to_string(), "Unknown".to_string()));

        Ok(WindowMetadata {
            title,
            process_path,
            process_name,
        })
    }

    fn get_window_title(&self, hwnd: HWND) -> Result<String> {
        unsafe {
            let length = GetWindowTextLengthW(hwnd);
            if length == 0 {
                return Ok("".to_string());
            }

            let mut buffer = vec![0u16; (length + 1) as usize];
            let read_len = GetWindowTextW(hwnd, &mut buffer);
            if read_len == 0 {
                return Ok("".to_string());
            }

            // check process encoding
            Ok(String::from_utf16_lossy(&buffer[..read_len as usize]))
        }
    }

    fn get_process_info(&self, hwnd: HWND) -> Result<(String, String)> {
        unsafe {
            let mut process_id = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut process_id));

            if process_id == 0 {
                return Err(anyhow::anyhow!("Failed to get process ID"));
            }

            let process_handle: HANDLE = OpenProcess(
                PROCESS_QUERY_INFORMATION | PROCESS_VM_READ,
                false,
                process_id,
            )?;

            if process_handle.is_invalid() {
                return Err(anyhow::anyhow!("Failed to open process"));
            }

            let mut buffer = vec![0u16; MAX_PATH as usize];
            let len = GetModuleFileNameExW(Some(process_handle), None, &mut buffer);

            if len == 0 {
                // close handle handled by Drop? No, OpenProcess returns HANDLE which is not automatically closed by windows-rs unless using specific wrapper?
                // windows-rs HANDLE implements Drop if it owns it. But raw HANDLE doesn't.
                // Owned<HANDLE> equivalent in windows crate is usually explicit.
                // Wait, windows crate 0.48+ HANDLE is Copy/Clone, so it doesn't close on drop.
                // We need to use Cancel/CloseHandle?
                // Actually windows::core::Owned is used sometimes, but standard HANDLE is raw.
                // Let's check docs or use `windows::Win32::Foundation::CloseHandle`.
                let _ = windows::Win32::Foundation::CloseHandle(process_handle);
                return Err(anyhow::anyhow!("Failed to get module file name"));
            }

            let full_path = String::from_utf16_lossy(&buffer[..len as usize]);
            let _ = windows::Win32::Foundation::CloseHandle(process_handle);

            let process_name = std::path::Path::new(&full_path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "Unknown".to_string());

            Ok((full_path, process_name))
        }
    }
}
