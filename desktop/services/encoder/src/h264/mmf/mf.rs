use anyhow::{Context, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::warn;
use windows::core::Array;
use windows::Win32::Media::MediaFoundation::{
    IMFActivate, IMFTransform, MFMediaType_Video, MFStartup, MFTEnumEx, MFVideoFormat_ARGB32,
    MFVideoFormat_H264, MFVideoFormat_NV12, MFSTARTUP_FULL, MFT_CATEGORY_VIDEO_ENCODER,
    MFT_ENUM_FLAG, MFT_ENUM_FLAG_ASYNCMFT, MFT_ENUM_FLAG_HARDWARE, MFT_REGISTER_TYPE_INFO,
};

// Media Foundationの初期化状態を管理（スレッドセーフ）
static MF_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Media Foundationを初期化（スレッドセーフ）
pub fn init_media_foundation() -> bool {
    if MF_INITIALIZED.load(Ordering::Acquire) {
        return true;
    }

    unsafe {
        match MFStartup(MFSTARTUP_FULL, 0) {
            Ok(_) => {
                MF_INITIALIZED.store(true, Ordering::Release);
                true
            }
            Err(e) => {
                warn!("Failed to initialize Media Foundation: {}", e);
                false
            }
        }
    }
}

fn enumerate_mfts(
    category: &windows::core::GUID,
    flags: MFT_ENUM_FLAG,
    input_type: Option<&MFT_REGISTER_TYPE_INFO>,
    output_type: Option<&MFT_REGISTER_TYPE_INFO>,
) -> Result<Vec<IMFActivate>> {
    let mut transform_sources = Vec::new();
    let mfactivate_list = unsafe {
        let mut data = std::ptr::null_mut();
        let mut len = 0;
        MFTEnumEx(
            *category,
            flags,
            input_type.map(|info| info as *const _),
            output_type.map(|info| info as *const _),
            &mut data,
            &mut len,
        )?;
        Array::<IMFActivate>::from_raw_parts(data as _, len)
    };
    if !mfactivate_list.is_empty() {
        for mfactivate in mfactivate_list.as_slice() {
            if let Some(transform_source) = mfactivate.clone() {
                transform_sources.push(transform_source);
            }
        }
    }
    Ok(transform_sources)
}

/// 非同期ハードウェア H.264 エンコーダー MFT を検索
pub unsafe fn find_async_h264_encoder() -> Result<IMFTransform> {
    let input_type = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_NV12,
    };

    let output_type = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_H264,
    };

    // 非同期ハードウェアエンコーダーを検索
    // 参考実装に合わせて SORTANDFILTER フラグを追加（より安定した選択のため）
    // 注意: windows-rs に SORTANDFILTER が定義されていない場合は、ビット値 0x00000001 を使用
    let mfactivate_list = enumerate_mfts(
        &MFT_CATEGORY_VIDEO_ENCODER, // guidCategory
        MFT_ENUM_FLAG(MFT_ENUM_FLAG_HARDWARE.0 | MFT_ENUM_FLAG_ASYNCMFT.0 | 0x00000001), // SORTANDFILTER
        Some(&input_type),
        Some(&output_type),
    )?;

    if mfactivate_list.is_empty() {
        return Err(anyhow::anyhow!("No async H.264 encoder MFT found"));
    }

    // 最初のMFTをアクティベート
    let activate = mfactivate_list
        .first()
        .ok_or_else(|| anyhow::anyhow!("No async H.264 encoder MFT found"))?;

    let transform: IMFTransform = activate
        .ActivateObject()
        .ok()
        .context("Failed to activate async H.264 encoder MFT")?;

    // 注意: ShutdownObject() を呼ぶと、ActivateObject() で取得した IMFTransform も無効化される
    // 参考実装では ShutdownObject() を呼んでいないため、ここでも呼ばない
    // Array の drop で適切にクリーンアップされる

    Ok(transform)
}

/// Video Processor MFT を検索
pub unsafe fn find_video_processor() -> Result<IMFTransform> {
    use windows::Win32::Media::MediaFoundation::MFT_CATEGORY_VIDEO_PROCESSOR;

    let input_type = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_ARGB32,
    };

    let output_type = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_NV12,
    };

    // Video Processor MFT を検索
    let mfactivate_list = enumerate_mfts(
        &MFT_CATEGORY_VIDEO_PROCESSOR,
        MFT_ENUM_FLAG(0x00000001), // SORTANDFILTER
        Some(&input_type),
        Some(&output_type),
    )?;

    if mfactivate_list.is_empty() {
        return Err(anyhow::anyhow!("No Video Processor MFT found"));
    }

    // 最初のMFTをアクティベート
    let activate = mfactivate_list
        .first()
        .ok_or_else(|| anyhow::anyhow!("No Video Processor MFT found"))?;

    let transform: IMFTransform = activate
        .ActivateObject()
        .ok()
        .context("Failed to activate Video Processor MFT")?;

    // 注意: ShutdownObject() を呼ぶと、ActivateObject() で取得した IMFTransform も無効化される
    // 参考実装では ShutdownObject() を呼んでいないため、ここでも呼ばない
    // Array の drop で適切にクリーンアップされる

    Ok(transform)
}

/// H.264エンコーダーMFTが存在するか確認（検索のみ）
pub unsafe fn find_h264_encoder() -> Result<()> {
    let input_type = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_NV12,
    };

    let output_type = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_H264,
    };

    // 非同期ハードウェアエンコーダーを検索
    // カテゴリを MFT_CATEGORY_VIDEO_ENCODER に修正（GUID::zeroed() は誤り）
    let mfactivate_list = enumerate_mfts(
        &MFT_CATEGORY_VIDEO_ENCODER,
        MFT_ENUM_FLAG(MFT_ENUM_FLAG_HARDWARE.0 | MFT_ENUM_FLAG_ASYNCMFT.0 | 0x00000001), // SORTANDFILTER
        Some(&input_type),
        Some(&output_type),
    )?;

    if mfactivate_list.is_empty() {
        return Err(anyhow::anyhow!("No H.264 encoder MFT found"));
    }

    // 注意: ShutdownObject() を呼ぶと、ActivateObject() で取得した IMFTransform も無効化される
    // この関数は検索のみなので、ShutdownObject() は不要
    // Array の drop で適切にクリーンアップされる

    Ok(())
}

/// Media Foundationが利用可能かチェック
pub fn check_mf_available() -> bool {
    // Media Foundationの初期化を試行
    if !init_media_foundation() {
        return false;
    }

    // D3D11デバイスが作成できるか確認
    if crate::h264::mmf::d3d::create_d3d11_device().is_err() {
        warn!("Failed to create D3D11 device");
        return false;
    }

    // H.264エンコーダーMFTが存在するか確認
    unsafe {
        match find_h264_encoder() {
            Ok(_) => {}
            Err(e) => {
                warn!("H.264 encoder MFT not found: {}", e);
                return false;
            }
        }
    }

    // Video Processor MFTが存在するか確認
    unsafe {
        match find_video_processor() {
            Ok(_) => {}
            Err(e) => {
                warn!("Video Processor MFT not found: {}", e);
                return false;
            }
        }
    }

    true
}
