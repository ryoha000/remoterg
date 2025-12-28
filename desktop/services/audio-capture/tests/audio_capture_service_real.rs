#[cfg(test)]
#[cfg(windows)]
mod tests {
    use anyhow::{Context, Result};
    use audio_capture::AudioCaptureService;
    use core_types::{AudioCaptureMessage, AudioFrame};
    use std::path::PathBuf;
    use std::sync::Once;
    use std::time::Duration;
    use tokio::sync::mpsc;
    use tokio::time::timeout;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::GetDesktopWindow;

    static INIT_TRACING: Once = Once::new();

    /// tracingを初期化（テスト実行時に一度だけ実行される）
    fn init_tracing() {
        INIT_TRACING.call_once(|| {
            tracing_subscriber::fmt()
                .with_max_level(tracing::Level::DEBUG)
                .with_test_writer()
                .init();
        });
    }

    /// テスト用のデスクトップウィンドウのHWNDを取得
    /// デスクトップウィンドウは常に存在するため、テストに適している
    unsafe fn get_desktop_window() -> HWND {
        GetDesktopWindow()
    }

    /// 音声フレームをWAVファイルとして保存
    fn save_audio_frames_as_wav(frames: &[AudioFrame]) -> Result<()> {
        if frames.is_empty() {
            return Err(anyhow::anyhow!("保存するフレームがありません"));
        }

        // 最初のフレームからサンプルレートとチャンネル数を取得
        let first_frame = &frames[0];
        let sample_rate = first_frame.sample_rate;
        let channels = first_frame.channels;

        // すべてのフレームが同じ設定であることを確認
        for frame in frames {
            if frame.sample_rate != sample_rate || frame.channels != channels {
                return Err(anyhow::anyhow!(
                    "フレーム間でサンプルレートまたはチャンネル数が一致しません"
                ));
            }
        }

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
        let filename = format!("audio_capture_{}.wav", timestamp);
        let filepath = artifacts_dir.join(&filename);

        // WAVファイルを作成
        let spec = hound::WavSpec {
            channels: channels as u16,
            sample_rate: sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };

        let mut writer = hound::WavWriter::create(&filepath, spec)
            .with_context(|| format!("WAVファイルの作成に失敗: {}", filepath.display()))?;

        // すべてのフレームのサンプルを書き込み
        for frame in frames {
            for sample in &frame.samples {
                writer
                    .write_sample(*sample)
                    .with_context(|| format!("サンプルの書き込みに失敗: {}", filepath.display()))?;
            }
        }

        writer
            .finalize()
            .with_context(|| format!("WAVファイルの最終化に失敗: {}", filepath.display()))?;

        println!("音声ファイルを保存しました: {}", filepath.display());
        println!("  サンプルレート: {} Hz", sample_rate);
        println!("  チャンネル数: {}", channels);
        println!("  フレーム数: {}", frames.len());
        println!(
            "  総サンプル数: {}",
            frames.iter().map(|f| f.samples.len()).sum::<usize>()
        );
        println!(
            "  録音時間: {:.2}秒",
            frames.iter().map(|f| f.samples.len()).sum::<usize>() as f64
                / (sample_rate as usize * channels as usize) as f64
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_audio_capture_service_real() -> Result<()> {
        init_tracing();

        // HWNDを取得（デスクトップウィンドウを使用）
        let hwnd = unsafe { get_desktop_window() };
        // let hwnd_raw = hwnd.0 as u64;
        let hwnd_raw = 1123566;
        println!("Using desktop window HWND: {}", hwnd_raw);

        // チャネルを作成
        let (frame_tx, mut frame_rx) = mpsc::channel(100);
        let (command_tx, command_rx) = mpsc::channel(10);

        // AudioCaptureServiceを起動
        let service = AudioCaptureService::new(frame_tx, command_rx);
        let service_handle = tokio::spawn(async move { service.run().await });

        // 録音を開始
        command_tx
            .send(AudioCaptureMessage::Start { hwnd: hwnd_raw })
            .await
            .unwrap();

        println!("録音を開始しました。5秒間録音します...");

        // 5秒間フレームを収集
        let mut frames = Vec::new();
        let start_time = std::time::Instant::now();
        let duration = Duration::from_secs(5);

        while start_time.elapsed() < duration {
            // タイムアウト付きでフレームを受信
            match timeout(Duration::from_millis(100), frame_rx.recv()).await {
                Ok(Some(frame)) => {
                    frames.push(frame);
                }
                Ok(None) => {
                    println!("フレームチャネルが閉じられました");
                    break;
                }
                Err(_) => {
                    // タイムアウト - 続行
                    continue;
                }
            }
        }

        println!(
            "フレーム収集完了。{}個のフレームを受信しました",
            frames.len()
        );

        // 録音を停止
        command_tx.send(AudioCaptureMessage::Stop).await.unwrap();

        // フレームの検証
        assert!(
            !frames.is_empty(),
            "少なくとも1つのフレームを受信できること"
        );

        // 最初のフレームで検証
        let first_frame = &frames[0];
        assert_eq!(
            first_frame.sample_rate, 48000,
            "サンプルレートは48000Hzであること"
        );
        assert_eq!(
            first_frame.channels, 2,
            "チャンネル数は2（ステレオ）であること"
        );
        assert!(
            first_frame.timestamp_us > 0,
            "タイムスタンプが正しく設定されていること"
        );

        // タイムスタンプが増加していることを確認
        let mut last_timestamp = first_frame.timestamp_us;
        for frame in frames.iter().skip(1) {
            assert!(
                frame.timestamp_us >= last_timestamp,
                "タイムスタンプが増加していること"
            );
            last_timestamp = frame.timestamp_us;
        }

        // 5秒間で約500フレーム（10ms/フレーム）が受信できることを確認
        // 実際には多少のばらつきがあるため、300フレーム以上あればOKとする
        assert!(
            frames.len() >= 300,
            "5秒間で少なくとも300フレームを受信できること（実際: {}フレーム）",
            frames.len()
        );

        println!(
            "フレーム検証成功: {}フレーム, サンプルレート: {}Hz, チャンネル数: {}",
            frames.len(),
            first_frame.sample_rate,
            first_frame.channels
        );

        // WAVファイルとして保存
        save_audio_frames_as_wav(&frames)?;

        // サービスを停止（チャネルを閉じる）
        drop(command_tx);

        // サービスが正常に終了することを確認（タイムアウト: 2秒）
        let _ = timeout(Duration::from_secs(2), service_handle).await;

        Ok(())
    }
}
