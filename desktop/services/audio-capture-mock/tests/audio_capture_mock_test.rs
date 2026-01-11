use anyhow::Result;
use audio_capture_mock::AudioCaptureService;
use core_types::{AudioCaptureMessage, AudioFrame};
use std::path::PathBuf;
use std::sync::Once;
use tokio::sync::mpsc;
use tokio::time::{timeout, Duration};

static INIT_TRACING: Once = Once::new();

fn init_tracing() {
    INIT_TRACING.call_once(|| {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_test_writer()
            .init();
    });
}

fn save_audio_frames_as_wav(frames: &[AudioFrame], filename: &str) -> Result<()> {
    let artifacts_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("artifacts");
    std::fs::create_dir_all(&artifacts_dir)?;

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let full_filename = format!("{}_{}.wav", filename, timestamp);
    let filepath = artifacts_dir.join(&full_filename);

    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: 48000,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };

    let mut writer = hound::WavWriter::create(&filepath, spec)?;

    for frame in frames {
        for sample in &frame.samples {
            writer.write_sample(*sample)?;
        }
    }

    writer.finalize()?;
    println!("Saved audio file: {}", filepath.display());
    Ok(())
}

#[tokio::test]
async fn test_audio_capture_mock_basic() -> Result<()> {
    init_tracing();

    let (frame_tx, mut frame_rx) = mpsc::channel(100);
    let (command_tx, command_rx) = mpsc::channel(10);

    let service = AudioCaptureService::new(frame_tx, command_rx);
    let service_handle = tokio::spawn(async move { service.run().await });

    // WAVファイル読み込み完了を待つ（最大5秒）
    tokio::time::sleep(Duration::from_secs(4)).await;

    // 録音開始
    command_tx
        .send(AudioCaptureMessage::Start { hwnd: 12345 })
        .await
        .unwrap();

    println!("Recording for 2 seconds...");

    // 2秒間フレームを収集
    let mut frames = Vec::new();
    let start_time = std::time::Instant::now();
    let duration = Duration::from_secs(2);

    while start_time.elapsed() < duration {
        match timeout(Duration::from_millis(100), frame_rx.recv()).await {
            Ok(Some(frame)) => {
                frames.push(frame);
            }
            Ok(None) => break,
            Err(_) => continue,
        }
    }

    println!("Collected {} frames", frames.len());

    // 検証
    assert!(!frames.is_empty(), "Should receive at least one frame");

    let first_frame = &frames[0];
    assert_eq!(first_frame.sample_rate, 48000);
    assert_eq!(first_frame.channels, 2);
    assert_eq!(first_frame.samples.len(), 960); // 480 * 2

    // 約200フレーム（2秒 / 10ms = 200）受信できることを確認
    // タイマー解像度を1msに設定したため、約200フレーム期待
    assert!(
        frames.len() >= 180,
        "Should receive at least 180 frames in 2 seconds (got {})",
        frames.len()
    );

    // タイムスタンプの単調増加を確認
    for i in 1..frames.len() {
        assert!(
            frames[i].timestamp_us > frames[i - 1].timestamp_us,
            "Timestamps should be monotonically increasing"
        );
    }

    // WAVファイルとして保存
    save_audio_frames_as_wav(&frames, "mock_capture")?;

    // 録音停止
    command_tx.send(AudioCaptureMessage::Stop).await.unwrap();
    drop(command_tx);

    let _ = timeout(Duration::from_secs(2), service_handle).await;

    println!("✓ Audio capture mock test passed");
    Ok(())
}

#[tokio::test]
async fn test_audio_capture_mock_loop() -> Result<()> {
    init_tracing();

    let (frame_tx, mut frame_rx) = mpsc::channel(100);
    let (command_tx, command_rx) = mpsc::channel(10);

    let service = AudioCaptureService::new(frame_tx, command_rx);
    let _service_handle = tokio::spawn(async move { service.run().await });

    // WAVファイル読み込み完了を待つ（最大5秒）
    tokio::time::sleep(Duration::from_secs(4)).await;

    command_tx
        .send(AudioCaptureMessage::Start { hwnd: 12345 })
        .await
        .unwrap();

    println!("Recording for 5 seconds to test loop...");

    // WAVファイルの全長を超えて録音（ループを確認）
    // 仮にWAVが1秒だとして、5秒間録音すれば5回ループする
    let mut frames = Vec::new();
    let start_time = std::time::Instant::now();
    let duration = Duration::from_secs(5);

    while start_time.elapsed() < duration {
        match timeout(Duration::from_millis(100), frame_rx.recv()).await {
            Ok(Some(frame)) => frames.push(frame),
            Ok(None) => break,
            Err(_) => continue,
        }
    }

    println!("Collected {} frames in 5 seconds", frames.len());

    // 約500フレーム受信できることを確認
    // タイマー解像度を1msに設定したため、約500フレーム期待
    assert!(
        frames.len() >= 450,
        "Should receive at least 450 frames in 5 seconds (got {})",
        frames.len()
    );

    // タイムスタンプがループしていないことを確認
    // （モックはリセットせず連続したタイムスタンプを送る）
    for i in 1..frames.len() {
        assert!(
            frames[i].timestamp_us >= frames[i - 1].timestamp_us,
            "Timestamps should be monotonically increasing (frame {} ts={}, frame {} ts={})",
            i - 1,
            frames[i - 1].timestamp_us,
            i,
            frames[i].timestamp_us
        );
    }

    // タイムスタンプの差がフレーム数 × 10ms と一致することを確認
    let time_diff_us = frames.last().unwrap().timestamp_us - frames.first().unwrap().timestamp_us;
    let time_diff_secs = time_diff_us as f64 / 1_000_000.0;
    let expected_time_secs = (frames.len() as f64 - 1.0) * 0.010; // (N-1) * 10ms
    println!(
        "Time span: {:.2} seconds (expected: {:.2}s for {} frames)",
        time_diff_secs,
        expected_time_secs,
        frames.len()
    );

    // タイムスタンプは10ms刻みなので、フレーム数から計算した値と一致すべき
    let time_diff_ratio = (time_diff_secs - expected_time_secs).abs() / expected_time_secs;
    assert!(
        time_diff_ratio < 0.05,
        "Timestamp span should match frame count * 10ms (got {:.2}s, expected {:.2}s)",
        time_diff_secs,
        expected_time_secs
    );

    drop(command_tx);

    println!("✓ Audio capture mock loop test passed");
    Ok(())
}
