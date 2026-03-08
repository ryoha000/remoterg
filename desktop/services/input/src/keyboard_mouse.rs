use crate::InputService;
use anyhow::Result;
use tracing::{debug, error};
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

impl InputService {
    pub(crate) async fn handle_mouse_click(&self, x: f64, y: f64, button: &str) -> Result<()> {
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

    pub(crate) async fn handle_cursor_move(&self, dx: i32, dy: i32) -> Result<()> {
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

    pub(crate) async fn handle_cursor_click(&self, button: &str) -> Result<()> {
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

    pub(crate) async fn handle_key_input(&self, key: &str, down: bool) -> Result<()> {
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

    pub(crate) fn map_to_virtual_screen(&self, x: i32, y: i32) -> (i32, i32) {
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
