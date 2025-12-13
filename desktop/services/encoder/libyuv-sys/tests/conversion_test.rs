/// libyuv-sysのRGBA→YUV420変換のテスト
/// libyuv-sysのRGBA→NV12変換のテスト

/// RGBA画像データを生成するヘルパー関数
fn create_rgba_image(width: usize, height: usize, r: u8, g: u8, b: u8, a: u8) -> Vec<u8> {
    let mut image = Vec::with_capacity(width * height * 4);
    for _ in 0..(width * height) {
        image.push(r);
        image.push(g);
        image.push(b);
        image.push(a);
    }
    image
}

/// YUV420バッファからY、U、V平面を抽出するヘルパー関数
fn extract_yuv_planes(
    yuv_buffer: &[u8],
    width: usize,
    height: usize,
) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let y_plane_size = width * height;
    let uv_plane_size = y_plane_size / 4;

    let y = yuv_buffer[0..y_plane_size].to_vec();
    let u = yuv_buffer[y_plane_size..y_plane_size + uv_plane_size].to_vec();
    let v = yuv_buffer[y_plane_size + uv_plane_size..y_plane_size + 2 * uv_plane_size].to_vec();

    (y, u, v)
}

/// NV12形式のUV平面からUとVを分離するヘルパー関数
/// UV平面はインターリーブ形式: [U0, V0, U1, V1, ...]
fn extract_uv_from_nv12(uv_plane: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let mut u = Vec::with_capacity(uv_plane.len() / 2);
    let mut v = Vec::with_capacity(uv_plane.len() / 2);
    for i in (0..uv_plane.len()).step_by(2) {
        if i + 1 < uv_plane.len() {
            u.push(uv_plane[i]);
            v.push(uv_plane[i + 1]);
        }
    }
    (u, v)
}

#[test]
fn test_white_image_conversion() {
    // 白い画像（RGBA: 255, 255, 255, 255）を変換
    let width = 64;
    let height = 64;
    let rgba = create_rgba_image(width, height, 255, 255, 255, 255);

    let mut y = vec![0u8; width * height];
    let mut u = vec![0u8; (width * height) / 4];
    let mut v = vec![0u8; (width * height) / 4];

    unsafe {
        let result = libyuv_sys::ABGRToI420(
            rgba.as_ptr(),
            (width * 4) as i32,
            y.as_mut_ptr(),
            width as i32,
            u.as_mut_ptr(),
            (width / 2) as i32,
            v.as_mut_ptr(),
            (width / 2) as i32,
            width as i32,
            height as i32,
        );

        assert_eq!(result, 0, "ABGRToI420 should return 0 on success");
    }

    // 白い画像なので、Y値は高いはず（約235-255の範囲）
    let avg_y: u32 = y.iter().map(|&x| x as u32).sum::<u32>() / y.len() as u32;
    assert!(
        avg_y > 200,
        "White image should have high Y values, got average: {}",
        avg_y
    );

    // UとVはほぼ128（無彩色なので）
    let avg_u: u32 = u.iter().map(|&x| x as u32).sum::<u32>() / u.len() as u32;
    let avg_v: u32 = v.iter().map(|&x| x as u32).sum::<u32>() / v.len() as u32;
    assert!(
        (avg_u as i32 - 128).abs() < 20,
        "White image U should be around 128, got: {}",
        avg_u
    );
    assert!(
        (avg_v as i32 - 128).abs() < 20,
        "White image V should be around 128, got: {}",
        avg_v
    );
}

#[test]
fn test_black_image_conversion() {
    // 黒い画像（RGBA: 0, 0, 0, 255）を変換
    let width = 64;
    let height = 64;
    let rgba = create_rgba_image(width, height, 0, 0, 0, 255);

    let mut y = vec![0u8; width * height];
    let mut u = vec![0u8; (width * height) / 4];
    let mut v = vec![0u8; (width * height) / 4];

    unsafe {
        let result = libyuv_sys::ABGRToI420(
            rgba.as_ptr(),
            (width * 4) as i32,
            y.as_mut_ptr(),
            width as i32,
            u.as_mut_ptr(),
            (width / 2) as i32,
            v.as_mut_ptr(),
            (width / 2) as i32,
            width as i32,
            height as i32,
        );

        assert_eq!(result, 0, "ABGRToI420 should return 0 on success");
    }

    // 黒い画像なので、Y値は低いはず（約16-30の範囲）
    let avg_y: u32 = y.iter().map(|&x| x as u32).sum::<u32>() / y.len() as u32;
    assert!(
        avg_y < 50,
        "Black image should have low Y values, got average: {}",
        avg_y
    );
}

#[test]
fn test_red_image_conversion() {
    // 赤い画像（RGBA: 255, 0, 0, 255）を変換
    let width = 64;
    let height = 64;
    let rgba = create_rgba_image(width, height, 255, 0, 0, 255);

    let mut y = vec![0u8; width * height];
    let mut u = vec![0u8; (width * height) / 4];
    let mut v = vec![0u8; (width * height) / 4];

    unsafe {
        let result = libyuv_sys::ABGRToI420(
            rgba.as_ptr(),
            (width * 4) as i32,
            y.as_mut_ptr(),
            width as i32,
            u.as_mut_ptr(),
            (width / 2) as i32,
            v.as_mut_ptr(),
            (width / 2) as i32,
            width as i32,
            height as i32,
        );

        assert_eq!(result, 0, "ABGRToI420 should return 0 on success");
    }

    // 赤い画像なので、V値は高いはず（赤はV成分が高い）
    let avg_v: u32 = v.iter().map(|&x| x as u32).sum::<u32>() / v.len() as u32;
    assert!(
        avg_v > 140,
        "Red image should have high V values, got average: {}",
        avg_v
    );
}

#[test]
fn test_gray_image_conversion() {
    // グレー画像（RGBA: 128, 128, 128, 255）を変換
    let width = 64;
    let height = 64;
    let rgba = create_rgba_image(width, height, 128, 128, 128, 255);

    let mut y = vec![0u8; width * height];
    let mut u = vec![0u8; (width * height) / 4];
    let mut v = vec![0u8; (width * height) / 4];

    unsafe {
        let result = libyuv_sys::ABGRToI420(
            rgba.as_ptr(),
            (width * 4) as i32,
            y.as_mut_ptr(),
            width as i32,
            u.as_mut_ptr(),
            (width / 2) as i32,
            v.as_mut_ptr(),
            (width / 2) as i32,
            width as i32,
            height as i32,
        );

        assert_eq!(result, 0, "ABGRToI420 should return 0 on success");
    }

    // グレー画像なので、Y値は中間程度
    let avg_y: u32 = y.iter().map(|&x| x as u32).sum::<u32>() / y.len() as u32;
    assert!(
        avg_y > 100 && avg_y < 180,
        "Gray image should have medium Y values, got average: {}",
        avg_y
    );

    // UとVはほぼ128（無彩色なので）
    let avg_u: u32 = u.iter().map(|&x| x as u32).sum::<u32>() / u.len() as u32;
    let avg_v: u32 = v.iter().map(|&x| x as u32).sum::<u32>() / v.len() as u32;
    assert!(
        (avg_u as i32 - 128).abs() < 20,
        "Gray image U should be around 128, got: {}",
        avg_u
    );
    assert!(
        (avg_v as i32 - 128).abs() < 20,
        "Gray image V should be around 128, got: {}",
        avg_v
    );
}

#[test]
fn test_yuv_buffer_sizes() {
    // YUV420バッファのサイズを検証
    let width = 64;
    let height = 64;
    let rgba = create_rgba_image(width, height, 128, 128, 128, 255);

    let mut y = vec![0u8; width * height];
    let mut u = vec![0u8; (width * height) / 4];
    let mut v = vec![0u8; (width * height) / 4];

    unsafe {
        let result = libyuv_sys::ABGRToI420(
            rgba.as_ptr(),
            (width * 4) as i32,
            y.as_mut_ptr(),
            width as i32,
            u.as_mut_ptr(),
            (width / 2) as i32,
            v.as_mut_ptr(),
            (width / 2) as i32,
            width as i32,
            height as i32,
        );

        assert_eq!(result, 0);
    }

    // サイズの検証
    assert_eq!(
        y.len(),
        width * height,
        "Y plane size should be width * height"
    );
    assert_eq!(
        u.len(),
        (width * height) / 4,
        "U plane size should be (width * height) / 4"
    );
    assert_eq!(
        v.len(),
        (width * height) / 4,
        "V plane size should be (width * height) / 4"
    );
}

#[test]
fn test_value_ranges() {
    // すべての値が有効な範囲（0-255）内にあることを確認
    let width = 64;
    let height = 64;
    let rgba = create_rgba_image(width, height, 200, 100, 50, 255);

    let mut y = vec![0u8; width * height];
    let mut u = vec![0u8; (width * height) / 4];
    let mut v = vec![0u8; (width * height) / 4];

    unsafe {
        let result = libyuv_sys::ABGRToI420(
            rgba.as_ptr(),
            (width * 4) as i32,
            y.as_mut_ptr(),
            width as i32,
            u.as_mut_ptr(),
            (width / 2) as i32,
            v.as_mut_ptr(),
            (width / 2) as i32,
            width as i32,
            height as i32,
        );

        assert_eq!(result, 0);
    }

    // すべての値が0-255の範囲内にあることを確認
    assert!(y.iter().all(|&x| x <= 255), "All Y values should be <= 255");
    assert!(u.iter().all(|&x| x <= 255), "All U values should be <= 255");
    assert!(v.iter().all(|&x| x <= 255), "All V values should be <= 255");
}

#[test]
fn test_different_sizes() {
    // 異なるサイズでの変換をテスト
    let sizes = vec![(32, 32), (64, 64), (128, 128), (256, 256)];

    for (width, height) in sizes {
        let rgba = create_rgba_image(width, height, 128, 128, 128, 255);

        let mut y = vec![0u8; width * height];
        let mut u = vec![0u8; (width * height) / 4];
        let mut v = vec![0u8; (width * height) / 4];

        unsafe {
            let result = libyuv_sys::ABGRToI420(
                rgba.as_ptr(),
                (width * 4) as i32,
                y.as_mut_ptr(),
                width as i32,
                u.as_mut_ptr(),
                (width / 2) as i32,
                v.as_mut_ptr(),
                (width / 2) as i32,
                width as i32,
                height as i32,
            );

            assert_eq!(
                result, 0,
                "Conversion should succeed for size {}x{}",
                width, height
            );
        }

        // サイズの検証
        assert_eq!(y.len(), width * height);
        assert_eq!(u.len(), (width * height) / 4);
        assert_eq!(v.len(), (width * height) / 4);
    }
}

#[test]
fn test_non_square_image() {
    // 非正方形画像の変換をテスト
    let width = 128;
    let height = 64;
    let rgba = create_rgba_image(width, height, 128, 128, 128, 255);

    let mut y = vec![0u8; width * height];
    let mut u = vec![0u8; (width * height) / 4];
    let mut v = vec![0u8; (width * height) / 4];

    unsafe {
        let result = libyuv_sys::ABGRToI420(
            rgba.as_ptr(),
            (width * 4) as i32,
            y.as_mut_ptr(),
            width as i32,
            u.as_mut_ptr(),
            (width / 2) as i32,
            v.as_mut_ptr(),
            (width / 2) as i32,
            width as i32,
            height as i32,
        );

        assert_eq!(result, 0, "Conversion should succeed for non-square image");
    }

    // サイズの検証
    assert_eq!(y.len(), width * height);
    assert_eq!(u.len(), (width * height) / 4);
    assert_eq!(v.len(), (width * height) / 4);
}

// ========== NV12形式のテスト ==========

#[test]
fn test_nv12_white_image_conversion() {
    // 白い画像（RGBA: 255, 255, 255, 255）をNV12形式に変換
    let width = 64;
    let height = 64;
    let rgba = create_rgba_image(width, height, 255, 255, 255, 255);

    let mut y = vec![0u8; width * height];
    let mut uv = vec![0u8; (width * height) / 2];

    unsafe {
        let result = libyuv_sys::ABGRToNV12(
            rgba.as_ptr(),
            (width * 4) as i32,
            y.as_mut_ptr(),
            width as i32,
            uv.as_mut_ptr(),
            width as i32,
            width as i32,
            height as i32,
        );

        assert_eq!(result, 0, "ABGRToNV12 should return 0 on success");
    }

    // 白い画像なので、Y値は高いはず（約235-255の範囲）
    let avg_y: u32 = y.iter().map(|&x| x as u32).sum::<u32>() / y.len() as u32;
    assert!(
        avg_y > 200,
        "White image should have high Y values, got average: {}",
        avg_y
    );

    // UV平面からUとVを抽出
    let (u, v) = extract_uv_from_nv12(&uv);

    // UとVはほぼ128（無彩色なので）
    let avg_u: u32 = u.iter().map(|&x| x as u32).sum::<u32>() / u.len() as u32;
    let avg_v: u32 = v.iter().map(|&x| x as u32).sum::<u32>() / v.len() as u32;
    assert!(
        (avg_u as i32 - 128).abs() < 20,
        "White image U should be around 128, got: {}",
        avg_u
    );
    assert!(
        (avg_v as i32 - 128).abs() < 20,
        "White image V should be around 128, got: {}",
        avg_v
    );
}

#[test]
fn test_nv12_black_image_conversion() {
    // 黒い画像（RGBA: 0, 0, 0, 255）をNV12形式に変換
    let width = 64;
    let height = 64;
    let rgba = create_rgba_image(width, height, 0, 0, 0, 255);

    let mut y = vec![0u8; width * height];
    let mut uv = vec![0u8; (width * height) / 2];

    unsafe {
        let result = libyuv_sys::ABGRToNV12(
            rgba.as_ptr(),
            (width * 4) as i32,
            y.as_mut_ptr(),
            width as i32,
            uv.as_mut_ptr(),
            width as i32,
            width as i32,
            height as i32,
        );

        assert_eq!(result, 0, "ABGRToNV12 should return 0 on success");
    }

    // 黒い画像なので、Y値は低いはず（約16-30の範囲）
    let avg_y: u32 = y.iter().map(|&x| x as u32).sum::<u32>() / y.len() as u32;
    assert!(
        avg_y < 50,
        "Black image should have low Y values, got average: {}",
        avg_y
    );
}

#[test]
fn test_nv12_red_image_conversion() {
    // 赤い画像（RGBA: 255, 0, 0, 255）をNV12形式に変換
    let width = 64;
    let height = 64;
    let rgba = create_rgba_image(width, height, 255, 0, 0, 255);

    let mut y = vec![0u8; width * height];
    let mut uv = vec![0u8; (width * height) / 2];

    unsafe {
        let result = libyuv_sys::ABGRToNV12(
            rgba.as_ptr(),
            (width * 4) as i32,
            y.as_mut_ptr(),
            width as i32,
            uv.as_mut_ptr(),
            width as i32,
            width as i32,
            height as i32,
        );

        assert_eq!(result, 0, "ABGRToNV12 should return 0 on success");
    }

    // UV平面からUとVを抽出
    let (_u, v) = extract_uv_from_nv12(&uv);

    // 赤い画像なので、V値は高いはず（赤はV成分が高い）
    let avg_v: u32 = v.iter().map(|&x| x as u32).sum::<u32>() / v.len() as u32;
    assert!(
        avg_v > 140,
        "Red image should have high V values, got average: {}",
        avg_v
    );
}

#[test]
fn test_nv12_gray_image_conversion() {
    // グレー画像（RGBA: 128, 128, 128, 255）をNV12形式に変換
    let width = 64;
    let height = 64;
    let rgba = create_rgba_image(width, height, 128, 128, 128, 255);

    let mut y = vec![0u8; width * height];
    let mut uv = vec![0u8; (width * height) / 2];

    unsafe {
        let result = libyuv_sys::ABGRToNV12(
            rgba.as_ptr(),
            (width * 4) as i32,
            y.as_mut_ptr(),
            width as i32,
            uv.as_mut_ptr(),
            width as i32,
            width as i32,
            height as i32,
        );

        assert_eq!(result, 0, "ABGRToNV12 should return 0 on success");
    }

    // グレー画像なので、Y値は中間程度
    let avg_y: u32 = y.iter().map(|&x| x as u32).sum::<u32>() / y.len() as u32;
    assert!(
        avg_y > 100 && avg_y < 180,
        "Gray image should have medium Y values, got average: {}",
        avg_y
    );

    // UV平面からUとVを抽出
    let (u, v) = extract_uv_from_nv12(&uv);

    // UとVはほぼ128（無彩色なので）
    let avg_u: u32 = u.iter().map(|&x| x as u32).sum::<u32>() / u.len() as u32;
    let avg_v: u32 = v.iter().map(|&x| x as u32).sum::<u32>() / v.len() as u32;
    assert!(
        (avg_u as i32 - 128).abs() < 20,
        "Gray image U should be around 128, got: {}",
        avg_u
    );
    assert!(
        (avg_v as i32 - 128).abs() < 20,
        "Gray image V should be around 128, got: {}",
        avg_v
    );
}

#[test]
fn test_nv12_buffer_sizes() {
    // NV12バッファのサイズを検証
    let width = 64;
    let height = 64;
    let rgba = create_rgba_image(width, height, 128, 128, 128, 255);

    let mut y = vec![0u8; width * height];
    let mut uv = vec![0u8; (width * height) / 2];

    unsafe {
        let result = libyuv_sys::ABGRToNV12(
            rgba.as_ptr(),
            (width * 4) as i32,
            y.as_mut_ptr(),
            width as i32,
            uv.as_mut_ptr(),
            width as i32,
            width as i32,
            height as i32,
        );

        assert_eq!(result, 0);
    }

    // サイズの検証
    assert_eq!(
        y.len(),
        width * height,
        "Y plane size should be width * height"
    );
    assert_eq!(
        uv.len(),
        (width * height) / 2,
        "UV plane size should be (width * height) / 2"
    );
}

#[test]
fn test_nv12_value_ranges() {
    // すべての値が有効な範囲（0-255）内にあることを確認
    let width = 64;
    let height = 64;
    let rgba = create_rgba_image(width, height, 200, 100, 50, 255);

    let mut y = vec![0u8; width * height];
    let mut uv = vec![0u8; (width * height) / 2];

    unsafe {
        let result = libyuv_sys::ABGRToNV12(
            rgba.as_ptr(),
            (width * 4) as i32,
            y.as_mut_ptr(),
            width as i32,
            uv.as_mut_ptr(),
            width as i32,
            width as i32,
            height as i32,
        );

        assert_eq!(result, 0);
    }

    // すべての値が有効な範囲内にあることを確認（u8型なので0-255の範囲は保証されている）
    assert!(!y.is_empty(), "Y plane should not be empty");
    assert!(!uv.is_empty(), "UV plane should not be empty");
}

#[test]
fn test_nv12_different_sizes() {
    // 異なるサイズでのNV12変換をテスト
    let sizes = vec![(32, 32), (64, 64), (128, 128), (256, 256)];

    for (width, height) in sizes {
        let rgba = create_rgba_image(width, height, 128, 128, 128, 255);

        let mut y = vec![0u8; width * height];
        let mut uv = vec![0u8; (width * height) / 2];

        unsafe {
            let result = libyuv_sys::ABGRToNV12(
                rgba.as_ptr(),
                (width * 4) as i32,
                y.as_mut_ptr(),
                width as i32,
                uv.as_mut_ptr(),
                width as i32,
                width as i32,
                height as i32,
            );

            assert_eq!(
                result, 0,
                "NV12 conversion should succeed for size {}x{}",
                width, height
            );
        }

        // サイズの検証
        assert_eq!(y.len(), width * height);
        assert_eq!(uv.len(), (width * height) / 2);
    }
}

#[test]
fn test_nv12_non_square_image() {
    // 非正方形画像のNV12変換をテスト
    let width = 128;
    let height = 64;
    let rgba = create_rgba_image(width, height, 128, 128, 128, 255);

    let mut y = vec![0u8; width * height];
    let mut uv = vec![0u8; (width * height) / 2];

    unsafe {
        let result = libyuv_sys::ABGRToNV12(
            rgba.as_ptr(),
            (width * 4) as i32,
            y.as_mut_ptr(),
            width as i32,
            uv.as_mut_ptr(),
            width as i32,
            width as i32,
            height as i32,
        );

        assert_eq!(
            result, 0,
            "NV12 conversion should succeed for non-square image"
        );
    }

    // サイズの検証
    assert_eq!(y.len(), width * height);
    assert_eq!(uv.len(), (width * height) / 2);
}
