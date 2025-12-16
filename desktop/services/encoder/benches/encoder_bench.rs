use core_types::{EncodeJob, VideoEncoderFactory};
use criterion::async_executor::AsyncExecutor;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use std::hint::black_box;
use std::time::{Duration, Instant};

#[cfg(feature = "h264")]
use encoder::h264::openh264::OpenH264EncoderFactory;

#[cfg(all(feature = "h264", windows))]
use encoder::h264::mmf::MediaFoundationH264EncoderFactory;

/// フレーム生成パターンの種類
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
    frame_count: usize,
    is_send_batch: bool,
) {
    let pattern_str = pattern_name(pattern);
    let benchmark_id = BenchmarkId::from_parameter(format!(
        "{}x{}_{}_{}frames",
        width, height, pattern_str, frame_count
    ));

    // 事前にフレームデータを生成
    let rgba_data = generate_rgba_data(width, height, pattern);

    let mut group = c.benchmark_group(format!("encode_{}_multiple", encoder_name));
    group.sample_size(10);
    group.bench_with_input(benchmark_id, &rgba_data, move |b, rgba| {
        // iter_batchedを使用: エンコーダーを一度だけ初期化し、その後フレームを連続してエンコード
        b.to_async(tokio::runtime::Runtime::new().unwrap())
            .iter(|| async {
                let (job_tx, mut res_rx) = factory.setup();

                // 測定対象: フレームエンコードのみ（初期化済みのエンコーダーを使用）
                if is_send_batch {
                    for i in 0..frame_count {
                        let job = EncodeJob {
                            width: black_box(width),
                            height: black_box(height),
                            rgba: black_box(rgba.clone()),
                            duration: black_box(Duration::from_millis(33)),
                            enqueue_at: black_box(Instant::now()),
                        };
                        job_tx.send(job).unwrap();
                    }
                    for i in 0..frame_count {
                        res_rx.recv().await.unwrap();
                    }
                } else {
                    for i in 0..frame_count {
                        let job = EncodeJob {
                            width: black_box(width),
                            height: black_box(height),
                            rgba: black_box(rgba.clone()),
                            duration: black_box(Duration::from_millis(33)),
                            enqueue_at: black_box(Instant::now()),
                        };
                        job_tx.send(job).unwrap();
                        res_rx.recv().await.unwrap();
                    }
                }
            });
    });
    group.finish();
}

#[cfg(feature = "h264")]
fn bench_openh264(c: &mut Criterion) {
    let factory = OpenH264EncoderFactory::new();

    // 複数フレームの連続エンコード（1080pのみ、代表的なパターン）
    bench_encoder_multiple_frames(
        c,
        "openh264",
        &factory,
        1920,
        1080,
        FramePattern::Gradient,
        100,
        false,
    );
    // bench_encoder_multiple_frames(
    //     c,
    //     "openh264",
    //     &factory,
    //     1920,
    //     1080,
    //     FramePattern::Realistic,
    //     10,
    // );
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
    bench_encoder_multiple_frames(
        c,
        "mmf",
        &factory,
        1920,
        1080,
        FramePattern::Gradient,
        100,
        true,
    );
    // bench_encoder_multiple_frames(c, "mmf", &factory, 1920, 1080, FramePattern::Realistic, 10);
}

#[cfg(all(feature = "h264", windows))]
// criterion_group!(benches, bench_openh264, bench_mmf);
criterion_group!(benches, bench_mmf);

#[cfg(all(feature = "h264", not(windows)))]
criterion_group!(benches, bench_openh264);

#[cfg(not(feature = "h264"))]
criterion_group!(benches);

criterion_main!(benches);
