use tracing::debug;
use windows::Win32::Media::MediaFoundation::{IMFTransform, MF_MT_MPEG_SEQUENCE_HEADER};

use crate::windows::h264::nal::parse_avc_decoder_config;

/// 出力メディアタイプからcodec config (SPS/PPS) を取得（best-effort）
/// 戻り値: (SPS NAL, PPS NAL) - 取得できない場合はNone
pub fn get_codec_config(transform: &IMFTransform) -> Option<(Vec<u8>, Vec<u8>)> {
    unsafe {
        // 出力CurrentTypeを取得
        let output_type = match transform.GetOutputCurrentType(0) {
            Ok(t) => t,
            Err(_) => {
                debug!("MF encoder: failed to get output current type for codec config");
                return None;
            }
        };

        // 適切なサイズのバッファを割り当ててGetBlobを試す
        // AVCDecoderConfigurationRecordは通常数百バイト程度
        let mut blob_data = vec![0u8; 512];
        let mut blob_len = blob_data.len() as u32;

        match output_type.GetBlob(
            &MF_MT_MPEG_SEQUENCE_HEADER,
            &mut blob_data,
            Some(&mut blob_len),
        ) {
            Ok(_) if blob_len > 0 && blob_len <= blob_data.len() as u32 => {
                blob_data.truncate(blob_len as usize);

                // AVCDecoderConfigurationRecordを解析
                if let Some((sps, pps)) = parse_avc_decoder_config(&blob_data) {
                    debug!("MF encoder: extracted SPS/PPS from codec config (SPS: {} bytes, PPS: {} bytes)", sps.len(), pps.len());
                    return Some((sps, pps));
                } else {
                    debug!("MF encoder: failed to parse AVCDecoderConfigurationRecord");
                }
            }
            Err(e) => {
                debug!(
                    "MF encoder: failed to get codec config blob: {} (HRESULT: {:?})",
                    e,
                    e.code()
                );
            }
            _ => {
                debug!("MF encoder: codec config not available or invalid size");
            }
        }

        None
    }
}
