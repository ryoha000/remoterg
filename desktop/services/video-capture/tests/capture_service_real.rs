use anyhow::Result;
use core_types::{CaptureBackend, CaptureMessage, Frame};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use video_capture::CaptureService;

/// GetScreenshot を使用してスクリーンショットを取得するテスト
#[tokio::test]
async fn test_screenshot_functionality() -> Result<()> {
    // チャネルを作成
    let (frame_tx, _frame_rx) = mpsc::channel::<Frame>(10);
    let (command_tx, command_rx) = mpsc::channel(10);

    // CaptureServiceを起動
    let service = CaptureService::new(frame_tx, command_rx);
    let _service_handle = tokio::spawn(async move { service.run().await });

    // キャプチャ可能なウィンドウを探す
    use windows_capture::window::Window;
    let windows =
        Window::enumerate().map_err(|e| anyhow::anyhow!("Failed to enumerate windows: {:?}", e))?;

    let hwnd = if let Some(window) = windows.first() {
        println!("Using window: {}", window.title().unwrap_or_default());
        window.as_raw_hwnd() as u64
    } else {
        println!("No windows found, test cannot proceed");
        return Ok(());
    };

    // キャプチャを開始
    command_tx.send(CaptureMessage::Start { hwnd }).await?;

    // 少し待機してキャプチャが安定するのを待つ
    tokio::time::sleep(Duration::from_millis(500)).await;

    // GetScreenshot でスクリーンショットを取得
    let (screenshot_tx, screenshot_rx) = oneshot::channel();
    command_tx
        .send(CaptureMessage::GetScreenshot { tx: screenshot_tx })
        .await?;

    // スクリーンショットを受信
    let screenshot = tokio::time::timeout(Duration::from_secs(5), screenshot_rx).await??;

    println!(
        "Screenshot received: {}x{}, data size: {} bytes",
        screenshot.width,
        screenshot.height,
        screenshot.data.len()
    );

    // スクリーンショットのデータを検証
    assert!(screenshot.width > 0, "Width should be greater than 0");
    assert!(screenshot.height > 0, "Height should be greater than 0");
    assert!(
        !screenshot.data.is_empty(),
        "Screenshot data should not be empty"
    );

    let expected_size = (screenshot.width * screenshot.height * 4) as usize;
    assert_eq!(
        screenshot.data.len(),
        expected_size,
        "Screenshot data size mismatch"
    );

    // キャプチャを停止
    command_tx.send(CaptureMessage::Stop).await?;

    Ok(())
}

/// texture_handle が設定されていることを確認するテスト
#[tokio::test]
async fn test_frame_has_texture_handle() -> Result<()> {
    // チャネルを作成
    let (frame_tx, mut frame_rx) = mpsc::channel::<Frame>(10);
    let (command_tx, command_rx) = mpsc::channel(10);

    // CaptureServiceを起動
    let service = CaptureService::new(frame_tx, command_rx);
    let _service_handle = tokio::spawn(async move { service.run().await });

    // キャプチャ可能なウィンドウを探す
    use windows_capture::window::Window;
    let windows =
        Window::enumerate().map_err(|e| anyhow::anyhow!("Failed to enumerate windows: {:?}", e))?;

    let hwnd = if let Some(window) = windows.first() {
        println!("Using window: {}", window.title().unwrap_or_default());
        window.as_raw_hwnd() as u64
    } else {
        println!("No windows found, test cannot proceed");
        return Ok(());
    };

    // キャプチャを開始
    command_tx.send(CaptureMessage::Start { hwnd }).await?;

    // フレームを受信
    let frame = tokio::time::timeout(Duration::from_secs(5), frame_rx.recv())
        .await?
        .expect("Frame should be received");

    println!(
        "Frame received: {}x{}, texture_handle: {:?}",
        frame.width, frame.height, frame.texture_handle
    );

    // texture_handle が設定されていることを確認
    assert!(
        frame.texture_handle.is_some(),
        "Frame should have texture_handle"
    );

    // キャプチャを停止
    command_tx.send(CaptureMessage::Stop).await?;

    Ok(())
}
