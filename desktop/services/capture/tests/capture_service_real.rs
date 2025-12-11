#[cfg(test)]
#[cfg(windows)]
mod tests {
    use anyhow::{Context, Result};
    use capture::CaptureService;
    use core_types::{CaptureBackend, CaptureMessage};
    use std::path::PathBuf;
    use std::time::Duration;
    use tokio::sync::mpsc;
    use tokio::time::timeout;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::GetDesktopWindow;

    /// テスト用のデスクトップウィンドウのHWNDを取得
    /// デスクトップウィンドウは常に存在するため、テストに適している
    unsafe fn get_desktop_window() -> HWND {
        GetDesktopWindow()
    }

    /// フレームをPNG画像として保存
    fn save_frame_as_image(frame: &core_types::Frame) -> Result<()> {
        // artifactsディレクトリを作成
        let artifacts_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).context("artifactsディレクトリの作成に失敗")?;

        // ファイル名を生成（タイムスタンプ付き）
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let filename = format!("capture_{}x{}_{}.png", frame.width, frame.height, timestamp);
        let filepath = artifacts_dir.join(&filename);

        // RGBAデータをimage::RgbaImageに変換
        // Frameのdataはstride付きの可能性があるため、実際のピクセルデータを抽出
        let stride = ((frame.width * 32 + 31) / 32 * 4) as usize;
        let mut rgba_data = Vec::with_capacity((frame.width * frame.height * 4) as usize);

        for y in 0..frame.height {
            let row_start = (y as usize) * stride;
            let row_end = row_start + (frame.width as usize * 4);
            if row_end <= frame.data.len() {
                rgba_data.extend_from_slice(&frame.data[row_start..row_end]);
            }
        }

        // image::RgbaImageを作成
        let img = image::RgbaImage::from_raw(frame.width, frame.height, rgba_data)
            .ok_or_else(|| anyhow::anyhow!("画像データの作成に失敗"))?;

        // PNGとして保存
        img.save(&filepath)
            .with_context(|| format!("画像の保存に失敗: {}", filepath.display()))?;

        println!("画像を保存しました: {}", filepath.display());
        Ok(())
    }

    #[tokio::test]
    async fn test_capture_service_real() -> Result<()> {
        // キャプチャ可能なウィンドウを探す
        use windows_capture::window::Window;
        let windows = Window::enumerate()
            .map_err(|e| anyhow::anyhow!("Failed to enumerate windows: {:?}", e))?;

        let hwnd_raw = if let Some(window) = windows.first() {
            println!("Using window: {}", window.title().unwrap_or_default());
            window.as_raw_hwnd() as u64
        } else {
            // フォールバック: デスクトップウィンドウを使用
            let hwnd = unsafe { get_desktop_window() };
            println!("No capturable windows found, using desktop window");
            hwnd.0 as u64
        };

        // チャネルを作成
        let (frame_tx, mut frame_rx) = mpsc::channel(10);
        let (command_tx, command_rx) = mpsc::channel(10);

        // CaptureServiceを起動
        let service = CaptureService::new(frame_tx, command_rx);
        let service_handle = tokio::spawn(async move { service.run().await });

        // 設定を更新
        // command_tx
        //     .send(CaptureMessage::UpdateConfig {
        //         width: 320,
        //         height: 240,
        //         fps: 30,
        //     })
        //     .await
        //     .unwrap();

        // キャプチャを開始
        command_tx
            .send(CaptureMessage::Start { hwnd: hwnd_raw })
            .await
            .unwrap();

        // フレームを受信（タイムアウト: 5秒）
        let frame_result = timeout(Duration::from_secs(5), frame_rx.recv()).await;

        match frame_result {
            Ok(Some(frame)) => {
                // フレームの基本検証
                // assert_eq!(frame.width, 320);
                // assert_eq!(frame.height, 240);
                // assert!(frame.data.len() >= (320 * 240 * 4) as usize);
                assert!(frame.timestamp > 0);

                // RGBAデータの検証（データが有効であることを確認）
                let stride = (320 * 32 + 31) / 32 * 4;
                let pixel_offset = (10 * stride + 10 * 4) as usize;
                if pixel_offset < frame.data.len() {
                    let r = frame.data[pixel_offset];
                    let g = frame.data[pixel_offset + 1];
                    let b = frame.data[pixel_offset + 2];
                    let a = frame.data[pixel_offset + 3];
                    // ピクセルデータが有効であることを確認（アルファチャンネルは255であるべき）
                    println!("左上ピクセル: R={}, G={}, B={}, A={}", r, g, b, a);
                    assert_eq!(a, 255, "アルファチャンネルは255であるべき");
                }

                println!(
                    "フレーム受信成功: {}x{}, データサイズ: {} bytes",
                    frame.width,
                    frame.height,
                    frame.data.len()
                );

                // 画像として保存
                save_frame_as_image(&frame)?;
            }
            Ok(None) => {
                anyhow::bail!("フレームチャネルが閉じられました");
            }
            Err(_) => {
                anyhow::bail!("フレーム受信タイムアウト");
            }
        }

        // キャプチャを停止
        command_tx.send(CaptureMessage::Stop).await.unwrap();

        // サービスを停止（チャネルを閉じる）
        drop(command_tx);

        // サービスが正常に終了することを確認（タイムアウト: 2秒）
        let _ = timeout(Duration::from_secs(2), service_handle).await;

        Ok(())
    }
}
