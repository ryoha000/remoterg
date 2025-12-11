use anyhow::{bail, Context, Result};
use core_types::{
    CaptureBackend, CaptureCommandReceiver, CaptureConfig, CaptureFrameSender, CaptureFuture,
    CaptureMessage, Frame,
};
use std::mem::size_of;
use std::time::Instant;
use tokio::time::{sleep, Duration};
use tracing::{debug, info};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, SetStretchBltMode, StretchBlt, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS, HALFTONE, HBITMAP, HDC, SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};
use windows::Win32::Foundation::HMODULE;
use windows::core::PCSTR;

// PrintWindow flags
const PW_RENDERFULLCONTENT: u32 = 0x00000002;

// PrintWindow関数の型定義
type PrintWindowFn = unsafe extern "system" fn(
    hwnd: HWND,
    hdc: HDC,
    flags: u32,
) -> windows::Win32::Foundation::BOOL;

unsafe fn print_window(hwnd: HWND, hdc: HDC, flags: u32) -> Result<bool> {
    // user32.dllは既にロードされているはずなので、LoadLibraryAを使用
    // user32.dllは既にロードされているので、参照カウントが増えるだけ
    let user32 = LoadLibraryA(PCSTR::from_raw(b"user32.dll\0".as_ptr()))
        .map_err(|e| anyhow::anyhow!("Failed to load user32.dll: {:?}", e))?;
    let _lib_guard = LibraryGuard { module: user32 };

    let proc_name = PCSTR::from_raw(b"PrintWindow\0".as_ptr());
    let print_window_ptr = GetProcAddress(user32, proc_name);
    if print_window_ptr.is_none() {
        bail!("PrintWindow not found");
    }

    let print_window_fn: PrintWindowFn = std::mem::transmute(print_window_ptr.unwrap());
    Ok(print_window_fn(hwnd, hdc, flags).as_bool())
}

struct LibraryGuard {
    module: HMODULE,
}

impl Drop for LibraryGuard {
    fn drop(&mut self) {
        // user32.dllはシステムライブラリなので、FreeLibraryは呼ばなくても問題ない
        // ただし、参照カウントを減らすために呼ぶこともできる
        // windows-rsにはFreeLibraryがないため、ここでは何もしない
    }
}

/// 実キャプチャサービス（GDIによる HWND キャプチャ）
pub struct CaptureService {
    frame_tx: CaptureFrameSender,
    command_rx: CaptureCommandReceiver,
}

impl CaptureBackend for CaptureService {
    fn new(frame_tx: CaptureFrameSender, command_rx: CaptureCommandReceiver) -> Self {
        Self {
            frame_tx,
            command_rx,
        }
    }

    fn run(self) -> CaptureFuture {
        Box::pin(async move { self.run_inner().await })
    }
}

impl CaptureService {
    async fn run_inner(mut self) -> Result<()> {
        info!("CaptureService (real) started");

        let mut is_capturing = false;
        let mut target_hwnd: Option<u64> = None;
        let mut config = CaptureConfig::default();
        let mut last_frame_log = Instant::now();

        loop {
            tokio::select! {
                msg = self.command_rx.recv() => {
                    match msg {
                        Some(CaptureMessage::Start { hwnd }) => {
                            info!("Start capture for HWND: {hwnd}");
                            target_hwnd = Some(hwnd);
                            is_capturing = true;
                        }
                        Some(CaptureMessage::Stop) => {
                            info!("Stop capture");
                            is_capturing = false;
                        }
                        Some(CaptureMessage::UpdateConfig { width, height, fps }) => {
                            info!("Update config: {}x{} @ {}fps", width, height, fps);
                            config.size = if width == 0 || height == 0 {
                                core_types::CaptureSize::UseSourceSize
                            } else {
                                core_types::CaptureSize::Custom { width, height }
                            };
                            config.fps = fps.max(1);
                        }
                        None => {
                            debug!("Command channel closed");
                            break;
                        }
                    }
                }
                _ = sleep(Duration::from_millis(1000 / config.fps.max(1) as u64)) => {
                    if is_capturing {
                        if let Some(hwnd_raw) = target_hwnd {
                            let hwnd = HWND(hwnd_raw as *mut _);
                            let frame_start = Instant::now();
                            match Self::capture_frame(hwnd, &config) {
                                Ok(mut frame) => {
                                    // 実送出時刻で timestamp を更新
                                    frame.timestamp = std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap()
                                        .as_millis() as u64;
                                    let send_start = Instant::now();
                                    if let Err(e) = self.frame_tx.send(frame).await {
                                        tracing::error!("Failed to send frame: {}", e);
                                        break;
                                    }
                                    let send_dur = send_start.elapsed();
                                    let total_dur = frame_start.elapsed();

                                    if last_frame_log.elapsed().as_secs_f32() >= 5.0 {
                                        info!(
                                            "capture running (real): send={}ms total={}ms",
                                            send_dur.as_millis(),
                                            total_dur.as_millis(),
                                        );
                                        last_frame_log = Instant::now();
                                    }
                                }
                                Err(e) => {
                                    tracing::error!("Capture failed: {e:?}");
                                }
                            }
                        }
                    }
                }
            }
        }

        info!("CaptureService (real) stopped");
        Ok(())
    }

    fn capture_frame(hwnd: HWND, config: &CaptureConfig) -> Result<Frame> {
        unsafe {
            // クライアント領域のサイズを取得
            let mut rect = RECT::default();
            GetClientRect(hwnd, &mut rect).context("GetClientRect failed")?;
            let src_width = (rect.right - rect.left) as i32;
            let src_height = (rect.bottom - rect.top) as i32;
            if src_width <= 0 || src_height <= 0 {
                bail!("Invalid source size: {}x{}", src_width, src_height);
            }

            let (dst_width, dst_height) = match &config.size {
                core_types::CaptureSize::UseSourceSize => (src_width, src_height),
                core_types::CaptureSize::Custom { width, height } => {
                    (*width as i32, *height as i32)
                }
            };

            // スクリーンDCを取得（互換性のあるDCを作成するため）
            let hdc_screen = GetDC(HWND::default());
            if hdc_screen.0.is_null() {
                bail!("GetDC failed");
            }
            let _screen_guard = ScreenDcGuard { hdc: hdc_screen };

            // メモリDCを作成
            let hdc_mem = CreateCompatibleDC(hdc_screen);
            if hdc_mem.0.is_null() {
                bail!("CreateCompatibleDC failed");
            }
            let _mem_guard = MemDcGuard { hdc: hdc_mem };

            // ビットマップを作成
            let hbitmap = CreateCompatibleBitmap(hdc_screen, dst_width, dst_height);
            if hbitmap.0.is_null() {
                bail!("CreateCompatibleBitmap failed");
            }
            let _bmp_guard = BitmapGuard { hbitmap };

            let old_obj = SelectObject(hdc_mem, hbitmap);
            if old_obj.0.is_null() {
                bail!("SelectObject failed");
            }

            // スケーリングが必要な場合は、まず元のサイズでキャプチャしてからリサイズ
            if dst_width != src_width || dst_height != src_height {
                // 一時的なメモリDCとビットマップを作成（元のサイズ用）
                let hdc_temp = CreateCompatibleDC(hdc_screen);
                if hdc_temp.0.is_null() {
                    bail!("CreateCompatibleDC failed for temp");
                }
                let _temp_guard = MemDcGuard { hdc: hdc_temp };

                let hbitmap_temp = CreateCompatibleBitmap(hdc_screen, src_width, src_height);
                if hbitmap_temp.0.is_null() {
                    bail!("CreateCompatibleBitmap failed for temp");
                }
                let _temp_bmp_guard = BitmapGuard { hbitmap: hbitmap_temp };

                let old_obj_temp = SelectObject(hdc_temp, hbitmap_temp);
                if old_obj_temp.0.is_null() {
                    bail!("SelectObject failed for temp");
                }

                // PrintWindowで元のサイズのビットマップにコピー
                // PW_RENDERFULLCONTENTフラグを使用して、完全なコンテンツをレンダリング
                if !print_window(hwnd, hdc_temp, PW_RENDERFULLCONTENT)? {
                    // PW_RENDERFULLCONTENTが失敗する場合（古いWindowsバージョンなど）、
                    // フラグなしで再試行
                    if !print_window(hwnd, hdc_temp, 0)? {
                        bail!("PrintWindow failed");
                    }
                }

                // 一時ビットマップから最終ビットマップにスケーリング
                SetStretchBltMode(hdc_mem, HALFTONE);
                if !StretchBlt(
                    hdc_mem, 0, 0, dst_width, dst_height,
                    hdc_temp, 0, 0, src_width, src_height,
                    SRCCOPY,
                )
                .as_bool()
                {
                    bail!("StretchBlt failed");
                }
            } else {
                // スケーリングが不要な場合は、直接PrintWindowを使用
                // PW_RENDERFULLCONTENTフラグを使用して、完全なコンテンツをレンダリング
                if !print_window(hwnd, hdc_mem, PW_RENDERFULLCONTENT)? {
                    // PW_RENDERFULLCONTENTが失敗する場合（古いWindowsバージョンなど）、
                    // フラグなしで再試行
                    if !print_window(hwnd, hdc_mem, 0)? {
                        bail!("PrintWindow failed");
                    }
                }
            }

            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: dst_width,
                    biHeight: -dst_height, // top-down
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                bmiColors: [Default::default(); 1],
            };

            let stride = (dst_width * 32 + 31) / 32 * 4;
            let mut data = vec![0u8; (stride * dst_height) as usize];

            let lines = GetDIBits(
                hdc_mem,
                hbitmap,
                0,
                dst_height as u32,
                Some(data.as_mut_ptr() as *mut _),
                &mut bmi,
                DIB_RGB_COLORS,
            );
            if lines == 0 {
                bail!("GetDIBits failed");
            }

            // BGRX -> RGBA
            for px in data.chunks_exact_mut(4) {
                let b = px[0];
                let g = px[1];
                let r = px[2];
                px[0] = r;
                px[1] = g;
                px[2] = b;
                px[3] = 255;
            }

            Ok(Frame {
                width: dst_width as u32,
                height: dst_height as u32,
                data,
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64,
            })
        }
    }
}

struct ScreenDcGuard {
    hdc: HDC,
}

impl Drop for ScreenDcGuard {
    fn drop(&mut self) {
        unsafe {
            ReleaseDC(HWND::default(), self.hdc);
        }
    }
}

struct MemDcGuard {
    hdc: HDC,
}

impl Drop for MemDcGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteDC(self.hdc);
        }
    }
}

struct BitmapGuard {
    hbitmap: HBITMAP,
}

impl Drop for BitmapGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.hbitmap);
        }
    }
}
