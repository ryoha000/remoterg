use anyhow::{Context, Result};
use windows::Win32::Media::MediaFoundation::{
    IMFTransform, MFMediaType_Video, MFVideoFormat_H264, MFVideoFormat_NV12,
    MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG, MFT_ENUM_FLAG_ASYNCMFT, MFT_ENUM_FLAG_HARDWARE,
    MFT_REGISTER_TYPE_INFO,
};

use crate::windows::utils::mf::enumerate_mfts;

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
        .context("Failed to activate async H.264 encoder MFT")?;

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
    let mfactivate_list = enumerate_mfts(
        &MFT_CATEGORY_VIDEO_ENCODER,
        MFT_ENUM_FLAG(MFT_ENUM_FLAG_HARDWARE.0 | MFT_ENUM_FLAG_ASYNCMFT.0 | 0x00000001), // SORTANDFILTER
        Some(&input_type),
        Some(&output_type),
    )?;

    if mfactivate_list.is_empty() {
        return Err(anyhow::anyhow!("No H.264 encoder MFT found"));
    }

    Ok(())
}

/// H.264エンコードのためのMedia Foundation環境が整っているかチェック
pub fn check_h264_mf_available() -> bool {
    // Media Foundationの基本機能チェック
    if !crate::windows::utils::mf::check_core_mf_available() {
        return false;
    }

    // H.264エンコーダーMFTが存在するか確認
    unsafe {
        match find_h264_encoder() {
            Ok(_) => {}
            Err(e) => {
                tracing::warn!("H.264 encoder MFT not found: {}", e);
                return false;
            }
        }
    }

    true
}
