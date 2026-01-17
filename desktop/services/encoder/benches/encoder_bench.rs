use core_types::{EncodeJob, EncodeResult, VideoEncoderFactory};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[cfg(feature = "h264")]
use encoder::h264::openh264::OpenH264EncoderFactory;

#[cfg(all(feature = "h264", windows))]
use encoder::h264::mmf::MediaFoundationH264EncoderFactory;

/// フレーム生成パターンの種類
#[allow(dead_code)]
#[derive(Clone, Copy)]
enum FramePattern {
    Gradient,
    Checker,
    Noise,
    Solid,
    Realistic,
}

/// グラデーションパターンのRGBAデータを生成
fn generate_gradient_rgba(width: u32, height: u32) -> Vec<u8> {
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let r = ((x * 255) / width.max(1)) as u8;
            let g = ((y * 255) / height.max(1)) as u8;
            let b = ((x + y) % 256) as u8;
            let a = 255u8;
            data.push(r);
            data.push(g);
            data.push(b);
            data.push(a);
        }
    }
    data
}

/// チェッカーパターンのRGBAデータを生成
fn generate_checker_rgba(width: u32, height: u32) -> Vec<u8> {
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    let tile_size = 32u32;
    for y in 0..height {
        for x in 0..width {
            let tile_x = (x / tile_size) % 2;
            let tile_y = (y / tile_size) % 2;
            let is_white = (tile_x + tile_y) % 2 == 0;
            let color = if is_white { 255u8 } else { 0u8 };
            data.push(color);
            data.push(color);
            data.push(color);
            data.push(255u8);
        }
    }
    data
}

/// ノイズパターンのRGBAデータを生成
fn generate_noise_rgba(width: u32, height: u32, seed: u32) -> Vec<u8> {
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    let mut rng = seed;
    for _ in 0..(width * height) {
        // 簡易的な線形合同法による疑似乱数生成
        rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
        let r = (rng >> 16) as u8;
        rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
        let g = (rng >> 16) as u8;
        rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
        let b = (rng >> 16) as u8;
        data.push(r);
        data.push(g);
        data.push(b);
        data.push(255u8);
    }
    data
}

/// 単色フレームのRGBAデータを生成
fn generate_solid_rgba(width: u32, height: u32, r: u8, g: u8, b: u8) -> Vec<u8> {
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    for _ in 0..(width * height) {
        data.push(r);
        data.push(g);
        data.push(b);
        data.push(255u8);
    }
    data
}

/// 実画像風パターンのRGBAデータを生成（複雑なパターン）
fn generate_realistic_rgba(width: u32, height: u32) -> Vec<u8> {
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            // 複数のサイン波を組み合わせた複雑なパターン
            let fx = x as f32 / width as f32;
            let fy = y as f32 / height as f32;

            let r = ((fx * 3.14159 * 4.0).sin() * 127.0 + 128.0) as u8;
            let g = ((fy * 3.14159 * 3.0).cos() * 127.0 + 128.0) as u8;
            let b = (((fx + fy) * 3.14159 * 5.0).sin() * 127.0 + 128.0) as u8;

            data.push(r);
            data.push(g);
            data.push(b);
            data.push(255u8);
        }
    }
    data
}

/// パターンに応じてRGBAデータを生成
fn generate_rgba_data(width: u32, height: u32, pattern: FramePattern) -> Vec<u8> {
    match pattern {
        FramePattern::Gradient => generate_gradient_rgba(width, height),
        FramePattern::Checker => generate_checker_rgba(width, height),
        FramePattern::Noise => generate_noise_rgba(width, height, 12345),
        FramePattern::Solid => generate_solid_rgba(width, height, 128, 128, 128),
        FramePattern::Realistic => generate_realistic_rgba(width, height),
    }
}

/// パターン名を文字列に変換
fn pattern_name(pattern: FramePattern) -> &'static str {
    match pattern {
        FramePattern::Gradient => "gradient",
        FramePattern::Checker => "checker",
        FramePattern::Noise => "noise",
        FramePattern::Solid => "solid",
        FramePattern::Realistic => "realistic",
    }
}

/// 複数フレームの連続エンコードベンチマーク
fn bench_encoder_multiple_frames<F: VideoEncoderFactory>(
    c: &mut Criterion,
    encoder_name: &str,
    factory: &F,
    width: u32,
    height: u32,
    pattern: FramePattern,
) {
    let pattern_str = pattern_name(pattern);
    let benchmark_id = BenchmarkId::from_parameter(format!("{}x{}_{}", width, height, pattern_str));

    // 1回の計測（イテレーション）で処理するフレーム数（バッチサイズ）
    // 動画エンコードは1フレームだと短すぎる場合があるため、ある程度まとめるのが一般的です
    let batch_size: u64 = 30;

    // 事前にフレームデータを生成
    let mut frames = Vec::new();
    for _ in 0..batch_size {
        let rgba_data = generate_rgba_data(width, height, pattern);
        frames.push(rgba_data);
    }

    let (job_slot, res_rx) = factory.setup();
    let res_rx = std::sync::Arc::new(tokio::sync::Mutex::new(res_rx));
    let input = (&frames, job_slot, res_rx);

    let mut group = c.benchmark_group(format!("encode_{}_multiple", encoder_name));
    // ★ここが重要: 単位を「要素数（Elements）」に設定
    group.throughput(Throughput::Elements(batch_size));
    // 長時間かかるベンチマークの警告を解消するため、target_timeを延長
    // 4Kエンコードは特に時間がかかるため、より長い時間を設定
    if width >= 3840 {
        group.measurement_time(Duration::from_secs(180)); // 4K: 3分
        group.sample_size(50); // サンプル数を減らして実行時間を短縮
    } else {
        group.measurement_time(Duration::from_secs(60)); // 1080p: 1分
        group.sample_size(100); // 1080pは100サンプル維持
    }
    group.bench_with_input(
        benchmark_id,
        &input,
        move |b,
              input: &(
            &Vec<Vec<u8>>,
            std::sync::Arc<core_types::EncodeJobSlot>,
            std::sync::Arc<tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<EncodeResult>>>,
        )| {
            // iter_batchedを使用: エンコーダーを一度だけ初期化し、その後フレームを連続してエンコード
            let job_slot = input.1.clone();
            let res_rx = input.2.clone();
            b.to_async(tokio::runtime::Runtime::new().unwrap())
                .iter(|| async {
                    // 測定対象: フレームエンコードのみ（初期化済みのエンコーダーを使用）
                    let mut rx = res_rx.lock().await;
                    for i in 0..batch_size {
                        // タイムスタンプは 33ms 間隔で設定
                        let timestamp = (i * 33) as u64;
                        let rgba = Arc::new(input.0[i as usize].clone());
                        let job = EncodeJob {
                            width: black_box(width),
                            height: black_box(height),
                            rgba: black_box(rgba),
                            timestamp: black_box(timestamp),
                            enqueue_at: black_box(Instant::now()),
                            request_keyframe: false,
                        };
                        job_slot.set(job);
                        rx.recv().await.unwrap();
                    }
                });
        },
    );
    group.finish();
}

#[cfg(feature = "h264")]
fn bench_openh264(c: &mut Criterion) {
    let factory = OpenH264EncoderFactory::new();

    // 複数フレームの連続エンコード（1080pのみ、代表的なパターン）
    bench_encoder_multiple_frames(c, "openh264", &factory, 1920, 1080, FramePattern::Noise);
    bench_encoder_multiple_frames(c, "openh264", &factory, 3840, 2160, FramePattern::Noise);
}

#[cfg(all(feature = "h264", windows))]
fn bench_mmf(c: &mut Criterion) {
    let factory = MediaFoundationH264EncoderFactory::new();

    // MMFが利用可能でない場合はスキップ
    if !factory.use_media_foundation() {
        eprintln!("Media Foundation is not available, skipping MMF benchmarks");
        return;
    }

    // 複数フレームの連続エンコード（1080pのみ、代表的なパターン）
    bench_encoder_multiple_frames(c, "mmf", &factory, 1920, 1080, FramePattern::Noise);
    bench_encoder_multiple_frames(c, "mmf", &factory, 3840, 2160, FramePattern::Noise);
}

#[cfg(all(feature = "h264", windows))]
criterion_group!(benches, bench_openh264, bench_mmf);

#[cfg(all(feature = "h264", not(windows)))]
criterion_group!(benches, bench_openh264);

#[cfg(not(feature = "h264"))]
criterion_group!(benches);

criterion_main!(benches);
