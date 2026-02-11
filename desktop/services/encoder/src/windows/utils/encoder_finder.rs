use anyhow::{Context, Result};
use core_types::VideoCodec;
use windows::Win32::Media::MediaFoundation::{
    IMFTransform, MFMediaType_Video, MFVideoFormat_AV1, MFVideoFormat_H264, MFVideoFormat_NV12,
    MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG, MFT_ENUM_FLAG_ASYNCMFT, MFT_ENUM_FLAG_HARDWARE,
    MFT_REGISTER_TYPE_INFO,
};

use super::mf::enumerate_mfts;

/// 非同期ハードウェアビデオエンコーダー MFT を検索（汎用）
///
/// # Arguments
/// * `codec` - ビデオコーデックの種類
pub unsafe fn find_async_video_encoder(codec: VideoCodec) -> Result<IMFTransform> {
    // VideoCodec から GUID へのマッピング
    let (output_subtype, codec_name) = match codec {
        VideoCodec::H264 => (MFVideoFormat_H264, "H.264"),
        VideoCodec::AV1 => (MFVideoFormat_AV1, "AV1"),
    };

    let input_type = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_NV12,
    };

    let output_type = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: output_subtype,
    };

    // 非同期ハードウェアエンコーダーを検索
    // SORTANDFILTER フラグ (0x00000001) を追加してより安定した選択を行う
    let mfactivate_list = enumerate_mfts(
        &MFT_CATEGORY_VIDEO_ENCODER,
        MFT_ENUM_FLAG(MFT_ENUM_FLAG_HARDWARE.0 | MFT_ENUM_FLAG_ASYNCMFT.0 | 0x00000001),
        Some(&input_type),
        Some(&output_type),
    )?;

    if mfactivate_list.is_empty() {
        return Err(anyhow::anyhow!("No {} hardware encoder found", codec_name));
    }

    // 最初のMFTをアクティベート
    let activate = mfactivate_list
        .first()
        .ok_or_else(|| anyhow::anyhow!("No {} hardware encoder found", codec_name))?;

    let transform: IMFTransform = activate
        .ActivateObject()
        .with_context(|| format!("Failed to activate {} encoder MFT", codec_name))?;

    Ok(transform)
}
