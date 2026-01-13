use core_types::Frame;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::sync::mpsc;
use video_capture::resize_image_impl;

/// ダミーのRGBAデータを生成する
fn generate_rgba_data(width: u32, height: u32) -> Vec<u8> {
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            // グラデーションのようなパターンを生成
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

fn bench_resize_image(c: &mut Criterion) {
    let mut group = c.benchmark_group("resize_image");

    // 1920x1080 → 1280x720
    group.bench_function("1920x1080_to_1280x720", |b| {
        let src_data = generate_rgba_data(1920, 1080);
        b.iter(|| {
            let result = resize_image_impl(
                black_box(&src_data),
                black_box(1920),
                black_box(1080),
                black_box(1280),
                black_box(720),
            );
            black_box(result)
        });
    });

    // 1920x1080 → 640x360
    group.bench_function("1920x1080_to_640x360", |b| {
        let src_data = generate_rgba_data(1920, 1080);
        b.iter(|| {
            let result = resize_image_impl(
                black_box(&src_data),
                black_box(1920),
                black_box(1080),
                black_box(640),
                black_box(360),
            );
            black_box(result)
        });
    });

    // 1920x1080 → 1920x1080（リサイズなし、コピーのみ）
    group.bench_function("1920x1080_to_1920x1080", |b| {
        let src_data = generate_rgba_data(1920, 1080);
        b.iter(|| {
            let result = resize_image_impl(
                black_box(&src_data),
                black_box(1920),
                black_box(1080),
                black_box(1920),
                black_box(1080),
            );
            black_box(result)
        });
    });

    // 3840x2160 → 1920x1080（4K→1080p）
    group.bench_function("3840x2160_to_1920x1080", |b| {
        let src_data = generate_rgba_data(3840, 2160);
        b.iter(|| {
            let result = resize_image_impl(
                black_box(&src_data),
                black_box(3840),
                black_box(2160),
                black_box(1920),
                black_box(1080),
            );
            black_box(result)
        });
    });

    group.finish();
}

fn bench_frame_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("frame_processing");

    // 1920x1080のフレーム処理
    group.bench_function("1920x1080_frame", |b| {
        let rgba_data = generate_rgba_data(1920, 1080);
        let (tx, _rx) = mpsc::channel();
        b.iter(|| {
            // Frame構造体の作成
            let frame = Frame {
                width: black_box(1920),
                height: black_box(1080),
                data: black_box(rgba_data.clone()),
                windows_timespan: black_box(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos() as u64
                        / 100,
                ),
            };
            // チャンネル送信（実際には送信しないが、構造体の作成を測定）
            let _ = tx.send(black_box(frame));
        });
    });

    // 1280x720のフレーム処理
    group.bench_function("1280x720_frame", |b| {
        let rgba_data = generate_rgba_data(1280, 720);
        let (tx, _rx) = mpsc::channel();
        b.iter(|| {
            let frame = Frame {
                width: black_box(1280),
                height: black_box(720),
                data: black_box(rgba_data.clone()),
                windows_timespan: black_box(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos() as u64
                        / 100,
                ),
            };
            let _ = tx.send(black_box(frame));
        });
    });

    // 640x360のフレーム処理
    group.bench_function("640x360_frame", |b| {
        let rgba_data = generate_rgba_data(640, 360);
        let (tx, _rx) = mpsc::channel();
        b.iter(|| {
            let frame = Frame {
                width: black_box(640),
                height: black_box(360),
                data: black_box(rgba_data.clone()),
                windows_timespan: black_box(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos() as u64
                        / 100,
                ),
            };
            let _ = tx.send(black_box(frame));
        });
    });

    group.finish();
}

criterion_group!(benches, bench_resize_image, bench_frame_processing);
criterion_main!(benches);
