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
            let hdc_window = GetDC(hwnd);
            if hdc_window.0.is_null() {
                bail!("GetDC failed");
            }
            let _hdc_guard = HdcGuard {
                hwnd,
                hdc: hdc_window,
            };

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

            let hdc_mem = CreateCompatibleDC(hdc_window);
            if hdc_mem.0.is_null() {
                bail!("CreateCompatibleDC failed");
            }
            let _mem_guard = MemDcGuard { hdc: hdc_mem };

            let hbitmap = CreateCompatibleBitmap(hdc_window, dst_width, dst_height);
            if hbitmap.0.is_null() {
                bail!("CreateCompatibleBitmap failed");
            }
            let _bmp_guard = BitmapGuard { hbitmap };

            let old_obj = SelectObject(hdc_mem, hbitmap);
            if old_obj.0.is_null() {
                bail!("SelectObject failed");
            }

            // 高品質スケーリングで StretchBlt
            SetStretchBltMode(hdc_mem, HALFTONE);
            if !StretchBlt(
                hdc_mem, 0, 0, dst_width, dst_height, hdc_window, 0, 0, src_width, src_height,
                SRCCOPY,
            )
            .as_bool()
            {
                bail!("StretchBlt failed");
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

struct HdcGuard {
    hwnd: HWND,
    hdc: HDC,
}

impl Drop for HdcGuard {
    fn drop(&mut self) {
        unsafe {
            ReleaseDC(self.hwnd, self.hdc);
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
