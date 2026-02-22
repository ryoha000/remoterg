use anyhow::{Context, Result};
use tracing::debug;
use windows::core::GUID;
use windows::Win32::Media::MediaFoundation::{
    CODECAPI_AVEncCommonLowLatency, CODECAPI_AVEncMPVDefaultBPictureCount,
    CODECAPI_AVLowLatencyMode, CODECAPI_AVEncAdaptiveMode, IMFMediaType, IMFTransform, MFCreateMediaType, MFMediaType_Video,
    MFVideoFormat_NV12, MFVideoInterlace_Progressive, MFT_SET_TYPE_TEST_ONLY,
    MF_E_INVALIDMEDIATYPE, MF_E_NO_MORE_TYPES, MF_LOW_LATENCY,
};

/// サポートされている入力解像度を検出
pub fn detect_supported_resolutions(transform: &IMFTransform) -> Result<Vec<(u32, u32)>> {
    unsafe {
        let mut supported_resolutions = Vec::new();
        let mut type_index = 0u32;

        loop {
            match transform.GetInputAvailableType(0, type_index) {
                Ok(mt) => {
                    // メジャータイプを確認
                    let major_type = mt
                        .GetGUID(&windows::Win32::Media::MediaFoundation::MF_MT_MAJOR_TYPE)
                        .context(format!(
                            "Failed to get input major type at index {}",
                            type_index
                        ))?;

                    if major_type == MFMediaType_Video {
                        // サブタイプを確認
                        let subtype = mt
                            .GetGUID(&windows::Win32::Media::MediaFoundation::MF_MT_SUBTYPE)
                            .context(format!(
                                "Failed to get input subtype at index {}",
                                type_index
                            ))?;

                        if subtype == MFVideoFormat_NV12 {
                            // フレームサイズを取得
                            if let Ok(frame_size) = mt.GetUINT64(
                                &windows::Win32::Media::MediaFoundation::MF_MT_FRAME_SIZE,
                            ) {
                                let w = (frame_size >> 32) as u32;
                                let h = (frame_size & 0xFFFFFFFF) as u32;
                                supported_resolutions.push((w, h));
                            }
                        }
                    }
                    type_index += 1;
                }
                Err(e) if e.code().0 == MF_E_NO_MORE_TYPES.0 => {
                    break;
                }
                Err(e) => {
                    // エラーが発生しても、これまでに取得した解像度を返す
                    debug!(
                        "Failed to enumerate input media types at index {}: {}",
                        type_index, e
                    );
                    break;
                }
            }
        }

        Ok(supported_resolutions)
    }
}

/// メディアタイプを設定
pub fn setup_media_types(
    transform: &IMFTransform,
    width: u32,
    height: u32,
    subtype: &GUID,
) -> Result<()> {
    unsafe {
        let frame_size = ((width as u64) << 32) | (height as u64);
        let frame_rate = (60u64 << 32) | 1u64;

        // 非同期MFTでは、出力メディアタイプを先に設定してから、
        // 入力メディアタイプを設定する必要がある
        // これにより、エンコーダーが出力形式を認識してから入力形式を受け入れることができる

        debug!("Setting output media type first");

        // 出力メディアタイプを列挙して指定された形式を探す
        debug!("Enumerating output media types for encoder");
        let mut output_media_type: Option<IMFMediaType> = None;
        let mut type_index = 0u32;

        loop {
            match transform.GetOutputAvailableType(0, type_index) {
                Ok(mt) => {
                    // メジャータイプを確認
                    let major_type = mt
                        .GetGUID(&windows::Win32::Media::MediaFoundation::MF_MT_MAJOR_TYPE)
                        .context(format!(
                            "Failed to get output major type at index {}",
                            type_index
                        ))?;

                    if major_type == MFMediaType_Video {
                        // サブタイプを確認
                        let current_subtype = mt
                            .GetGUID(&windows::Win32::Media::MediaFoundation::MF_MT_SUBTYPE)
                            .context(format!(
                                "Failed to get output subtype at index {}",
                                type_index
                            ))?;

                        debug!(
                            "Found output media type at index {}: major={:?}, subtype={:?}",
                            type_index, major_type, current_subtype
                        );

                        if current_subtype == *subtype {
                            debug!(
                                "Found matching output media type at index {} (subtype={:?})",
                                type_index, subtype
                            );
                            output_media_type = Some(mt);
                            break;
                        }
                    }
                    type_index += 1;
                }
                Err(e) if e.code().0 == MF_E_NO_MORE_TYPES.0 => {
                    debug!(
                        "No more output media types available after {} types",
                        type_index
                    );
                    break;
                }
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "Failed to enumerate output media types at index {}: {}",
                        type_index,
                        e
                    ));
                }
            }
        }

        let output_media_type = output_media_type.ok_or_else(|| {
            anyhow::anyhow!(
                "No matching output media type found after enumerating {} types",
                type_index
            )
        })?;

        // 列挙されたメディアタイプをコピーして新しいメディアタイプを作成
        let configured_output_type =
            MFCreateMediaType().context("Failed to create output media type for configuration")?;

        // 列挙されたメディアタイプからすべての属性をコピー
        output_media_type
            .CopyAllItems(&configured_output_type)
            .context("Failed to copy output media type attributes")?;

        // 必要な属性を設定
        configured_output_type
            .SetUINT64(
                &windows::Win32::Media::MediaFoundation::MF_MT_FRAME_SIZE,
                frame_size,
            )
            .context("Failed to set output frame size")?;

        configured_output_type
            .SetUINT64(
                &windows::Win32::Media::MediaFoundation::MF_MT_FRAME_RATE,
                frame_rate,
            )
            .context("Failed to set output frame rate")?;

        configured_output_type
            .SetUINT32(
                &windows::Win32::Media::MediaFoundation::MF_MT_INTERLACE_MODE,
                MFVideoInterlace_Progressive.0 as u32,
            )
            .context("Failed to set output interlace mode")?;

        // 出力メディアタイプを設定
        transform
            .SetOutputType(0, &configured_output_type, 0)
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to set encoder output type (width={}, height={}): {}",
                    width,
                    height,
                    e
                )
            })?;

        debug!("Output media type set successfully, now setting input type");

        // 入力メディアタイプを列挙して、サポートされているタイプを探す
        // 参考実装に従い、GetInputAvailableTypeで列挙し、MFT_SET_TYPE_TEST_ONLYでテストしてから設定
        let input_type: Option<IMFMediaType> =
            (|| -> windows::core::Result<Option<IMFMediaType>> {
                let mut count = 0u32;
                loop {
                    let result = transform.GetInputAvailableType(0, count);
                    match &result {
                        Err(error) if error.code().0 == MF_E_NO_MORE_TYPES.0 => {
                            break Ok(None);
                        }
                        Err(error) => {
                            return Err(error.clone());
                        }
                        Ok(_) => {}
                    }

                    let input_type = result?;

                    // メジャータイプとサブタイプを確認
                    let major_type = match input_type
                        .GetGUID(&windows::Win32::Media::MediaFoundation::MF_MT_MAJOR_TYPE)
                    {
                        Ok(guid) => guid,
                        Err(_) => {
                            count += 1;
                            continue;
                        }
                    };

                    if major_type != MFMediaType_Video {
                        count += 1;
                        continue;
                    }

                    let subtype = match input_type
                        .GetGUID(&windows::Win32::Media::MediaFoundation::MF_MT_SUBTYPE)
                    {
                        Ok(guid) => guid,
                        Err(_) => {
                            count += 1;
                            continue;
                        }
                    };

                    if subtype != MFVideoFormat_NV12 {
                        count += 1;
                        continue;
                    }

                    // 新しいメディアタイプを作成して設定を試みる
                    let configured_input_type = MFCreateMediaType()?;

                    // 列挙されたメディアタイプからすべての属性をコピー
                    input_type.CopyAllItems(&configured_input_type)?;

                    // 必要な属性を設定
                    configured_input_type.SetUINT64(
                        &windows::Win32::Media::MediaFoundation::MF_MT_FRAME_SIZE,
                        frame_size,
                    )?;

                    configured_input_type.SetUINT64(
                        &windows::Win32::Media::MediaFoundation::MF_MT_FRAME_RATE,
                        frame_rate,
                    )?;

                    configured_input_type.SetUINT32(
                        &windows::Win32::Media::MediaFoundation::MF_MT_INTERLACE_MODE,
                        MFVideoInterlace_Progressive.0 as u32,
                    )?;

                    // MFT_SET_TYPE_TEST_ONLYでテスト
                    let test_result = transform.SetInputType(
                        0,
                        &configured_input_type,
                        MFT_SET_TYPE_TEST_ONLY.0 as u32,
                    );

                    match &test_result {
                        Err(error) if error.code().0 == MF_E_INVALIDMEDIATYPE.0 => {
                            count += 1;
                            continue;
                        }
                        Err(error) => {
                            return Err(error.clone());
                        }
                        Ok(_) => {}
                    }

                    // テスト成功したら実際に設定
                    transform.SetInputType(0, &configured_input_type, 0)?;
                    break Ok(Some(configured_input_type));
                }
            })()
            .map_err(|e| {
                // サポートされている解像度を検出してエラーメッセージに含める
                let supported_resolutions =
                    detect_supported_resolutions(transform).unwrap_or_default();

                let resolution_info = if supported_resolutions.is_empty() {
                    "Unable to detect supported resolutions".to_string()
                } else {
                    let mut resolutions_str = String::new();
                    for (w, h) in supported_resolutions.iter().take(10) {
                        if !resolutions_str.is_empty() {
                            resolutions_str.push_str(", ");
                        }
                        resolutions_str.push_str(&format!("{}x{}", w, h));
                    }
                    if supported_resolutions.len() > 10 {
                        resolutions_str
                            .push_str(&format!(", ... ({} total)", supported_resolutions.len()));
                    }
                    format!("Supported resolutions include: {}", resolutions_str)
                };

                anyhow::anyhow!(
                    "Failed to set encoder input type (width={}, height={}): {}. {}",
                    width,
                    height,
                    e,
                    resolution_info
                )
            })?;

        if input_type.is_none() {
            return Err(anyhow::anyhow!(
                "No suitable input type found for {}x{}. Try a different resolution.",
                width,
                height
            ));
        }

        debug!("Input media type set successfully");

        Ok(())
    }
}

/// 低遅延属性を設定
pub fn setup_low_latency_attributes(transform: &IMFTransform) -> Result<()> {
    unsafe {
        // Attributes を取得
        let attributes = transform.GetAttributes()?;
        attributes
            .SetUINT32(&MF_LOW_LATENCY, 1)
            .map_err(|e| anyhow::anyhow!("Failed to set MF_LOW_LATENCY attribute: {}", e))?;
        attributes
            .SetUINT32(&CODECAPI_AVLowLatencyMode, 1)
            .map_err(|e| {
                anyhow::anyhow!("Failed to set CODECAPI_AVLowLatencyMode attribute: {}", e)
            })?;
        attributes
            .SetUINT32(&CODECAPI_AVEncCommonLowLatency, 1)
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to set CODECAPI_AVEncCommonLowLatency attribute: {}",
                    e
                )
            })?;
        attributes
            .SetUINT32(&CODECAPI_AVEncMPVDefaultBPictureCount, 0)
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to set CODECAPI_AVEncMPVDefaultBPictureCount attribute: {}",
                    e
                )
            })?;

        // 解像度優先 (eAVEncAdaptiveMode_Resolution = 1) に設定してテキスト等の鮮明さを維持
        // (非対応のエンコーダーもあるためエラーは無視する)
        if let Err(e) = attributes.SetUINT32(&CODECAPI_AVEncAdaptiveMode, 1) {
            tracing::debug!("CODECAPI_AVEncAdaptiveMode is not supported by this MFT: {}", e);
        }

        Ok(())
    }
}
