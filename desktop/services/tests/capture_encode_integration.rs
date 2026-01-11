#[cfg(test)]
#[cfg(windows)]
mod tests {
    use anyhow::{Context, Result};
    use capture::CaptureService;
    use core_types::{CaptureBackend, CaptureMessage, EncodeJob, Frame, VideoEncoderFactory};
    use encoder::h264::mmf::MediaFoundationH264EncoderFactory;
    use std::path::PathBuf;
    use std::sync::Once;
    use std::time::{Duration, Instant};
    use tokio::sync::mpsc as tokio_mpsc;
    use tokio::time::timeout;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::GetDesktopWindow;

    static INIT_TRACING: Once = Once::new();

    /// tracingを初期化（テスト実行時に一度だけ実行される）
    fn init_tracing() {
        INIT_TRACING.call_once(|| {
            tracing_subscriber::fmt()
                .with_max_level(tracing::Level::INFO)
                .with_test_writer()
                .init();
        });
    }

    /// テスト用のデスクトップウィンドウのHWNDを取得
    unsafe fn get_desktop_window() -> HWND {
        GetDesktopWindow()
    }

    /// エンコード結果をrawストリームとして保存
    fn save_encoded_stream(
        samples: &[Vec<u8>],
        width: u32,
        height: u32,
        actual_fps: f32,
        actual_duration_sec: f32,
        encoded_frame_count: usize,
        codec_name: &str,
        file_extension: &str,
    ) -> Result<()> {
        // artifactsディレクトリを作成
        let artifacts_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).context("artifactsディレクトリの作成に失敗")?;

        // ファイル名を生成（タイムスタンプ付き）
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let filename = format!(
            "capture_encode_{}x{}_{}.{}",
            width, height, timestamp, file_extension
        );
        let filepath = artifacts_dir.join(&filename);

        // ファイルに書き込み
        let mut file = std::fs::File::create(&filepath)
            .with_context(|| format!("ファイルの作成に失敗: {}", filepath.display()))?;

        use std::io::Write;

        // 生データをそのまま書き込み
        for sample in samples {
            file.write_all(sample)
                .with_context(|| format!("ファイルへの書き込みに失敗: {}", filepath.display()))?;
        }

        file.sync_all()
            .with_context(|| format!("ファイルの同期に失敗: {}", filepath.display()))?;

        println!(
            "{}ストリームを保存しました: {}",
            codec_name,
            filepath.display()
        );
        println!("  解像度: {}x{}", width, height);
        println!("  サンプル数: {}", samples.len());
        println!(
            "  総サイズ: {} bytes",
            samples.iter().map(|s| s.len()).sum::<usize>()
        );
        println!(
            "  注意: raw {}ストリームにはタイムスタンプ情報が含まれていません。",
            codec_name
        );
        println!(
            "  実際のキャプチャ時間: {:.2}秒, エンコードされたフレーム数: {}, 実際のFPS: {:.2}",
            actual_duration_sec, encoded_frame_count, actual_fps
        );
        // エンコードされたフレーム数と実際のキャプチャ時間から正確なフレームレートを計算
        // skip_framesがtrueの場合、一部のフレームがスキップされるため、エンコードされたフレーム数を使用
        let output_fps = if actual_duration_sec > 0.0 {
            encoded_frame_count as f32 / actual_duration_sec
        } else {
            actual_fps
        };

        // H.264の場合はMP4コンテナを使用
        if file_extension == "h264" || file_extension == "264" {
            let mp4_path = filepath
                .display()
                .to_string()
                .replace(file_extension, "mp4");
            println!("  ffmpegでMP4に変換する際は、以下のコマンドを使用してください:");
            println!(
                "  ffmpeg -r {:.2} -i {} -c:v copy -r {:.2} -y {}",
                output_fps,
                filepath.display(),
                output_fps,
                mp4_path
            );
        }
        println!(
            "  もし短い動画になっている場合、skip_framesがtrueのため一部のフレームがスキップされている可能性があります。"
        );
        println!(
            "  キャプチャフレーム数とエンコードフレーム数を比較して、スキップされたフレーム数を確認してください。"
        );

        Ok(())
    }

    /// フレームをキャプチャする共通処理
    async fn capture_frames(
        capture_duration: Duration,
    ) -> Result<(
        Vec<Frame>,
        u32,
        u32,
        f32,
        tokio::task::JoinHandle<Result<()>>,
    )> {
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
        let (frame_tx, mut frame_rx) = tokio_mpsc::channel(100);
        let (command_tx, command_rx) = tokio_mpsc::channel(10);

        // CaptureServiceを起動
        let service = CaptureService::new(frame_tx, command_rx);
        let service_handle = tokio::spawn(async move { service.run().await });

        // キャプチャを開始
        command_tx
            .send(CaptureMessage::Start { hwnd: hwnd_raw })
            .await
            .context("キャプチャ開始に失敗")?;

        println!(
            "キャプチャを開始しました。{}秒間フレームを収集します...",
            capture_duration.as_secs()
        );

        // フレームを収集
        let capture_start = Instant::now();
        let mut frames: Vec<Frame> = Vec::new();

        while capture_start.elapsed() < capture_duration {
            match timeout(Duration::from_millis(100), frame_rx.recv()).await {
                Ok(Some(frame)) => {
                    frames.push(frame);
                    if frames.len() % 30 == 0 {
                        println!(
                            "  収集済みフレーム数: {} (経過時間: {:.1}秒)",
                            frames.len(),
                            capture_start.elapsed().as_secs_f32()
                        );
                    }
                }
                Ok(None) => {
                    anyhow::bail!("フレームチャネルが閉じられました");
                }
                Err(_) => {
                    // タイムアウト - 続行
                    continue;
                }
            }
        }

        println!(
            "フレーム収集完了: {}フレームを{}秒で収集",
            frames.len(),
            capture_start.elapsed().as_secs()
        );

        if frames.is_empty() {
            anyhow::bail!("フレームが1つも収集されませんでした");
        }

        // フレームのタイムスタンプ情報を確認
        let first_frame = &frames[0];
        let last_frame = &frames[frames.len() - 1];
        // windows_timespan は100ナノ秒単位なので、ミリ秒に変換
        let delta_hns = last_frame
            .windows_timespan
            .saturating_sub(first_frame.windows_timespan);
        let actual_duration_ms = delta_hns / 10_000;
        let actual_duration_sec = actual_duration_ms as f32 / 1000.0;
        let avg_fps = if actual_duration_sec > 0.0 {
            frames.len() as f32 / actual_duration_sec
        } else {
            0.0
        };
        println!(
            "  最初のフレームタイムスタンプ: {} (100ns units)",
            first_frame.windows_timespan
        );
        println!(
            "  最後のフレームタイムスタンプ: {} (100ns units)",
            last_frame.windows_timespan
        );
        println!(
            "  実際のキャプチャ時間: {:.2}秒 (タイムスタンプ差分)",
            actual_duration_sec
        );
        println!("  平均FPS: {:.2}", avg_fps);

        // 最初のフレームの解像度を取得
        let width = first_frame.width;
        let height = first_frame.height;
        println!("解像度: {}x{}", width, height);

        // キャプチャを停止
        command_tx.send(CaptureMessage::Stop).await.unwrap();
        drop(command_tx);

        // CaptureServiceが停止するまで少し待つ
        tokio::time::sleep(Duration::from_millis(500)).await;

        Ok((frames, width, height, actual_duration_sec, service_handle))
    }

    /// キャプチャとエンコードをパイプライン化して実行（実運用に近い方式）
    async fn capture_and_encode_pipeline(
        encoder_factory: &dyn VideoEncoderFactory,
        capture_duration: Duration,
    ) -> Result<(
        Vec<Vec<u8>>,
        usize,
        Duration,
        u32,
        u32,
        f32,
        tokio::task::JoinHandle<Result<()>>,
    )> {
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
        let (frame_tx, mut frame_rx) = tokio_mpsc::channel(100);
        let (command_tx, command_rx) = tokio_mpsc::channel(10);

        // CaptureServiceを起動
        let service = CaptureService::new(frame_tx, command_rx);
        let service_handle = tokio::spawn(async move { service.run().await });

        // エンコーダーを初期化
        println!("エンコーダーを初期化中...");
        let (job_slot, encode_result_rx) = encoder_factory.setup();
        println!("エンコードワーカーを起動しました");

        // エンコード結果を収集するタスクを起動
        let (encode_samples_tx, mut encode_samples_rx) = tokio_mpsc::unbounded_channel::<Vec<u8>>();
        let mut encode_result_rx_clone = encode_result_rx;
        let encode_collector_handle = tokio::spawn(async move {
            let mut samples: Vec<Vec<u8>> = Vec::new();
            while let Some(sample) = encode_samples_rx.recv().await {
                samples.push(sample);
            }
            samples
        });

        // エンコード結果を受信するタスクを起動
        let encode_samples_tx_clone = encode_samples_tx.clone();
        let result_receiver_handle = tokio::spawn(async move {
            let mut count = 0;
            let mut total_duration = Duration::ZERO;
            let mut last_log = Instant::now();

            let receive_start = Instant::now();

            while let Some(result) = encode_result_rx_clone.recv().await {
                total_duration += result.duration;

                if encode_samples_tx_clone.send(result.sample_data).is_err() {
                    break;
                }
                count += 1;

                // 50フレームごと、または5秒ごとに進捗を表示
                if count % 50 == 0 || last_log.elapsed().as_secs() >= 5 {
                    let elapsed = receive_start.elapsed();
                    let throughput = count as f64 / elapsed.as_secs_f64();
                    println!(
                        "  受信済みエンコード結果: {}フレーム (経過: {:.1}s, スループット: {:.2} fps)",
                        count, elapsed.as_secs_f32(), throughput
                    );
                    last_log = Instant::now();
                }
            }
            let total_elapsed = receive_start.elapsed();
            println!(
                "  エンコード結果の受信完了: {}フレーム (総時間: {:.2}秒)",
                count,
                total_elapsed.as_secs_f32()
            );

            (count, total_duration)
        });

        // キャプチャを開始
        command_tx
            .send(CaptureMessage::Start { hwnd: hwnd_raw })
            .await
            .context("キャプチャ開始に失敗")?;

        println!(
            "キャプチャを開始しました。{}秒間フレームを収集・エンコードします...",
            capture_duration.as_secs()
        );

        // パイプライン処理: キャプチャしながら逐次エンコード
        let capture_start = Instant::now();
        let mut frame_count = 0;
        let mut last_frame_ts: Option<u64> = None;
        let mut first_frame: Option<Frame> = None;
        let mut last_frame: Option<Frame> = None;
        let mut width = 0u32;
        let mut height = 0u32;

        // キャプチャ期間中はフレームを受信して即座にエンコードジョブに送る
        while capture_start.elapsed() < capture_duration {
            match timeout(Duration::from_millis(100), frame_rx.recv()).await {
                Ok(Some(frame)) => {
                    if first_frame.is_none() {
                        first_frame = Some(frame.clone());
                        width = frame.width;
                        height = frame.height;
                        println!("解像度: {}x{}", width, height);
                    }
                    last_frame = Some(frame.clone());
                    frame_count += 1;

                    // タイムスタンプを更新（エンコーダー側で duration を計算するため、ここでは更新のみ）
                    last_frame_ts = Some(frame.windows_timespan);

                    // EncodeJobを作成して即座に送信
                    let job = EncodeJob {
                        width: frame.width,
                        height: frame.height,
                        rgba: frame.data,
                        timestamp: frame.windows_timespan,
                        enqueue_at: Instant::now(),
                        request_keyframe: false,
                    };

                    job_slot.set(job);

                    // 進捗表示
                    if frame_count % 30 == 0 {
                        println!(
                            "  キャプチャ・送信済み: {}フレーム (経過時間: {:.1}秒)",
                            frame_count,
                            capture_start.elapsed().as_secs_f32()
                        );
                    }
                }
                Ok(None) => {
                    anyhow::bail!("フレームチャネルが閉じられました");
                }
                Err(_) => {
                    // タイムアウト - 続行
                    continue;
                }
            }
        }

        println!(
            "キャプチャ完了: {}フレームを{}秒で収集",
            frame_count,
            capture_start.elapsed().as_secs()
        );

        if frame_count == 0 {
            anyhow::bail!("フレームが1つも収集されませんでした");
        }

        // フレームのタイムスタンプ情報を確認
        let first_frame_ts = first_frame.as_ref().unwrap().windows_timespan;
        let last_frame_ts_val = last_frame.as_ref().unwrap().windows_timespan;
        // windows_timespan は100ナノ秒単位なので、ミリ秒に変換
        let delta_hns = last_frame_ts_val.saturating_sub(first_frame_ts);
        let actual_duration_ms = delta_hns / 10_000;
        let actual_duration_sec = actual_duration_ms as f32 / 1000.0;
        let avg_fps = if actual_duration_sec > 0.0 {
            frame_count as f32 / actual_duration_sec
        } else {
            0.0
        };
        println!(
            "  最初のフレームタイムスタンプ: {} (100ns units)",
            first_frame_ts
        );
        println!(
            "  最後のフレームタイムスタンプ: {} (100ns units)",
            last_frame_ts_val
        );
        println!(
            "  実際のキャプチャ時間: {:.2}秒 (タイムスタンプ差分)",
            actual_duration_sec
        );
        println!("  平均FPS: {:.2}", avg_fps);

        // キャプチャを停止
        command_tx.send(CaptureMessage::Stop).await.unwrap();
        drop(command_tx);

        // CaptureServiceが停止するまで少し待つ
        tokio::time::sleep(Duration::from_millis(500)).await;

        println!("すべてのエンコードジョブを送信しました。結果を待機中...");

        // エンコード結果の受信タスクが完了するまで待つ
        let (encoded_count, total_video_duration) =
            timeout(Duration::from_secs(120), result_receiver_handle)
                .await
                .context("エンコード結果の受信がタイムアウト")?
                .context("エンコード結果の受信タスクが失敗")?;

        // エンコード結果の受信を停止（すべての結果が処理された後）
        drop(encode_samples_tx);

        // エンコードサンプルを収集
        let samples = timeout(Duration::from_secs(1), encode_collector_handle)
            .await
            .context("エンコードサンプルの収集がタイムアウト")?
            .context("エンコードサンプルの収集に失敗")?;

        println!("エンコード完了: {}フレームをエンコード", encoded_count);
        println!(
            "  キャプチャフレーム数: {}, エンコードフレーム数: {}",
            frame_count, encoded_count
        );
        println!(
            "  総動画再生時間（duration合計）: {:.2}秒",
            total_video_duration.as_secs_f32()
        );

        if samples.is_empty() {
            anyhow::bail!("エンコードされたサンプルが1つもありませんでした");
        }

        Ok((
            samples,
            encoded_count,
            total_video_duration,
            width,
            height,
            actual_duration_sec,
            service_handle,
        ))
    }

    /// フレームをエンコードする共通処理
    async fn encode_frames(
        encoder_factory: &dyn VideoEncoderFactory,
        frames: Vec<Frame>,
    ) -> Result<(Vec<Vec<u8>>, usize, Duration)> {
        println!("エンコーダーを初期化中...");

        // フレーム数を先に取得（後でframesをconsumeするため）
        let frame_count = frames.len();

        // エンコードワーカーを起動
        let (job_slot, encode_result_rx) = encoder_factory.setup();
        println!("エンコードワーカーを起動しました");

        // エンコード結果を収集するタスクを起動
        let (encode_samples_tx, mut encode_samples_rx) = tokio_mpsc::unbounded_channel::<Vec<u8>>();
        let mut encode_result_rx_clone = encode_result_rx;
        let encode_collector_handle = tokio::spawn(async move {
            let mut samples: Vec<Vec<u8>> = Vec::new();
            while let Some(sample) = encode_samples_rx.recv().await {
                samples.push(sample);
            }
            samples
        });

        // エンコード結果を受信するタスクを起動
        let encode_samples_tx_clone = encode_samples_tx.clone();
        let result_receiver_handle = tokio::spawn(async move {
            let mut count = 0;
            let mut total_duration = Duration::ZERO;
            let mut last_log = Instant::now();

            let receive_start = Instant::now();

            while let Some(result) = encode_result_rx_clone.recv().await {
                total_duration += result.duration;

                if encode_samples_tx_clone.send(result.sample_data).is_err() {
                    break;
                }
                count += 1;

                // 50フレームごと、または5秒ごとに進捗を表示
                if count % 50 == 0 || last_log.elapsed().as_secs() >= 5 {
                    let elapsed = receive_start.elapsed();
                    let throughput = count as f64 / elapsed.as_secs_f64();
                    println!(
                        "  受信済みエンコード結果: {}フレーム (経過: {:.1}s, スループット: {:.2} fps)",
                        count, elapsed.as_secs_f32(), throughput
                    );
                    last_log = Instant::now();
                }
            }
            let total_elapsed = receive_start.elapsed();
            println!(
                "  エンコード結果の受信完了: {}フレーム (総時間: {:.2}秒)",
                count,
                total_elapsed.as_secs_f32()
            );

            (count, total_duration)
        });

        // フレームを順次エンコード
        println!("フレームをエンコード中...");
        let encode_start = Instant::now();
        let mut last_frame_ts: Option<u64> = None;

        for (idx, frame) in frames.into_iter().enumerate() {
            // タイムスタンプを更新（エンコーダー側で duration を計算するため、ここでは更新のみ）
            last_frame_ts = Some(frame.windows_timespan);

            // EncodeJobを作成（frame.dataをmoveで渡す）
            let job = EncodeJob {
                width: frame.width,
                height: frame.height,
                rgba: frame.data, // clone()を削除してmove
                timestamp: frame.windows_timespan,
                enqueue_at: Instant::now(),
                request_keyframe: false,
            };

            job_slot.set(job);

            // 進捗表示
            if (idx + 1) % 100 == 0 {
                println!(
                    "  送信済み: {}フレーム (経過時間: {:.1}秒)",
                    idx + 1,
                    encode_start.elapsed().as_secs_f32()
                );
            }
        }

        // すべてのジョブを送信したことを示すため、少し待つ
        println!("すべてのエンコードジョブを送信しました。結果を待機中...");

        println!("エンコード結果の受信を開始します...");

        // エンコード結果の受信タスクが完了するまで待つ（encode_result_rxが閉じられるまで）
        let (encoded_count, total_video_duration) =
            timeout(Duration::from_secs(120), result_receiver_handle)
                .await
                .context("エンコード結果の受信がタイムアウト")?
                .context("エンコード結果の受信タスクが失敗")?;

        // エンコード結果の受信を停止（すべての結果が処理された後）
        drop(encode_samples_tx);

        // エンコードサンプルを収集
        let samples = timeout(Duration::from_secs(1), encode_collector_handle)
            .await
            .context("エンコードサンプルの収集がタイムアウト")?
            .context("エンコードサンプルの収集に失敗")?;

        println!(
            "エンコード完了: {}フレームを{}秒でエンコード",
            encoded_count,
            encode_start.elapsed().as_secs_f32()
        );
        println!(
            "  キャプチャフレーム数: {}, エンコードフレーム数: {}",
            frame_count, encoded_count
        );
        println!(
            "  総動画再生時間（duration合計）: {:.2}秒",
            total_video_duration.as_secs_f32()
        );

        if samples.is_empty() {
            anyhow::bail!("エンコードされたサンプルが1つもありませんでした");
        }

        Ok((samples, encoded_count, total_video_duration))
    }

    #[tokio::test]
    async fn test_capture_encode_integration_h264() -> Result<()> {
        init_tracing();
        // エンコーダーファクトリを作成（Media Foundation H.264エンコーダーを使用）
        let encoder_factory = MediaFoundationH264EncoderFactory::new();

        // パイプライン化: キャプチャしながら逐次エンコード
        let capture_duration = Duration::from_secs(8);
        let (
            samples,
            encoded_count,
            total_video_duration,
            width,
            height,
            actual_duration_sec,
            service_handle,
        ) = capture_and_encode_pipeline(&encoder_factory, capture_duration).await?;

        // 統計情報を出力
        println!(
            "  実際のキャプチャ時間（タイムスタンプ差分）: {:.2}秒",
            actual_duration_sec
        );
        println!(
            "  プレーヤーが推測する再生時間（30fps想定）: {:.2}秒",
            encoded_count as f32 / 30.0
        );
        println!(
            "  プレーヤーが推測する再生時間（60fps想定）: {:.2}秒",
            encoded_count as f32 / 60.0
        );

        // durationの合計と実際のキャプチャ時間を比較
        if total_video_duration.as_secs_f32() < actual_duration_sec * 0.8 {
            println!(
                "  警告: duration合計 ({:.2}秒) が実際のキャプチャ時間 ({:.2}秒) より大幅に短いです。",
                total_video_duration.as_secs_f32(),
                actual_duration_sec
            );
            println!("  フレーム間隔の計算に問題がある可能性があります。");
        }

        // 実際のフレームレートを計算
        let actual_fps = if actual_duration_sec > 0.0 {
            encoded_count as f32 / actual_duration_sec
        } else {
            0.0
        };

        // H.264ストリームとして保存
        save_encoded_stream(
            &samples,
            width,
            height,
            actual_fps,
            actual_duration_sec,
            encoded_count,
            "H.264",
            "h264",
        )?;

        // サービスが正常に終了することを確認
        let _ = timeout(Duration::from_secs(2), service_handle).await;

        Ok(())
    }
}
