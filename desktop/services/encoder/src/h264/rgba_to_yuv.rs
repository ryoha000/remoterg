/// RGBA形式の画像データをYUV420形式に変換する（libyuv使用）
///
/// # Arguments
/// * `rgba` - RGBA画像データ（元のサイズ）
/// * `width` - エンコード用の幅（2の倍数）
/// * `height` - エンコード用の高さ（2の倍数）
/// * `src_width` - 元のRGBAデータの幅
///
/// # Returns
/// YUV420バッファ（`3 * width * height / 2`バイト）
/// レイアウト: Y平面 + U平面 + V平面
pub fn rgba_to_yuv420(rgba: &[u8], width: usize, height: usize, src_width: usize) -> Vec<u8> {
    let y_plane_size = width * height;
    let uv_plane_size = y_plane_size / 4;
    let total_size = y_plane_size + 2 * uv_plane_size;

    let mut y = vec![0u8; y_plane_size];
    let mut u = vec![0u8; uv_plane_size];
    let mut v = vec![0u8; uv_plane_size];

    // libyuvのABGRToI420を使用
    // ABGRはメモリ上ではRGBAと同じ順序（R, G, B, A）
    unsafe {
        let result = libyuv_sys::ABGRToI420(
            rgba.as_ptr(),
            (src_width * 4) as i32, // src_stride: RGBAなので4バイト/ピクセル
            y.as_mut_ptr(),
            width as i32, // dst_stride_y
            u.as_mut_ptr(),
            (width / 2) as i32, // dst_stride_u
            v.as_mut_ptr(),
            (width / 2) as i32, // dst_stride_v
            width as i32,
            height as i32,
        );

        if result != 0 {
            // エラーが発生した場合は警告を出す
            // ただし、通常はエラーにならないはず
            tracing::warn!("libyuv ABGRToI420 failed with error code: {}", result);
        }
    }

    let mut buffer = Vec::with_capacity(total_size);
    buffer.extend_from_slice(&y);
    buffer.extend_from_slice(&u);
    buffer.extend_from_slice(&v);
    buffer
}
