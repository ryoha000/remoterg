use anyhow::{Context, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::warn;
use windows::core::Array;
use windows::Win32::Media::MediaFoundation::{
    IMFActivate, IMFTransform, MFMediaType_Video, MFStartup, MFTEnumEx, MFVideoFormat_ARGB32,
    MFVideoFormat_NV12, MFSTARTUP_FULL, MFT_CATEGORY_VIDEO_PROCESSOR, MFT_ENUM_FLAG,
    MFT_REGISTER_TYPE_INFO,
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

pub fn enumerate_mfts(
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

/// Video Processor MFT を検索
pub unsafe fn find_video_processor() -> Result<IMFTransform> {
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
        .context("Failed to activate Video Processor MFT")?;

    Ok(transform)
}
