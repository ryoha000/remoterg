use tracing::{debug, warn};

/// H.264データがAnnex-B形式（スタートコード）かどうかを判定
pub fn is_annexb_format(data: &[u8]) -> bool {
    if data.len() < 4 {
        return false;
    }
    // 4バイトスタートコード (00 00 00 01)
    if data[0] == 0x00 && data[1] == 0x00 && data[2] == 0x00 && data[3] == 0x01 {
        return true;
    }
    // 3バイトスタートコード (00 00 01)
    if data.len() >= 3 && data[0] == 0x00 && data[1] == 0x00 && data[2] == 0x01 {
        return true;
    }
    false
}

/// H.264データをAnnex-B形式に変換（フォーマット自動判定）
/// 戻り値: (Annex-B形式のデータ, SPS/PPSが含まれているか)
pub fn annexb_from_mf_data(data: &[u8]) -> (Vec<u8>, bool) {
    const START_CODE: &[u8] = &[0x00, 0x00, 0x00, 0x01];
    let mut result = Vec::new();
    let mut has_sps_pps = false;

    // 既にAnnex-B形式の場合はそのまま返す
    if is_annexb_format(data) {
        // Annex-B形式のまま処理（NALユニットを分割してSPS/PPSを検出）
        let mut i = 0;
        while i < data.len() {
            // スタートコードを探す
            let start_code_len = if i + 4 <= data.len()
                && data[i] == 0x00
                && data[i + 1] == 0x00
                && data[i + 2] == 0x00
                && data[i + 3] == 0x01
            {
                4
            } else if i + 3 <= data.len()
                && data[i] == 0x00
                && data[i + 1] == 0x00
                && data[i + 2] == 0x01
            {
                3
            } else {
                // スタートコードが見つからない場合は残りをコピーして終了
                if i < data.len() {
                    result.extend_from_slice(&data[i..]);
                }
                break;
            };

            // 次のスタートコードを探す
            let mut next_start = None;
            let mut search_pos = i + start_code_len;
            while search_pos + 3 <= data.len() {
                if search_pos + 4 <= data.len()
                    && data[search_pos] == 0x00
                    && data[search_pos + 1] == 0x00
                    && data[search_pos + 2] == 0x00
                    && data[search_pos + 3] == 0x01
                {
                    next_start = Some((search_pos, 4));
                    break;
                } else if data[search_pos] == 0x00
                    && data[search_pos + 1] == 0x00
                    && data[search_pos + 2] == 0x01
                {
                    next_start = Some((search_pos, 3));
                    break;
                }
                search_pos += 1;
            }

            let nal_end = next_start.unwrap_or((data.len(), 0)).0;
            let nal_unit = &data[i..nal_end];

            // NALユニットのタイプを確認（SPS/PPS判定）
            if nal_unit.len() > start_code_len {
                let nal_header = nal_unit[start_code_len];
                let nal_type = nal_header & 0x1F;
                if nal_type == 7 || nal_type == 8 {
                    has_sps_pps = true;
                    debug!(
                        "MF encoder: found SPS/PPS in Annex-B data (type={})",
                        nal_type
                    );
                }
            }

            result.extend_from_slice(nal_unit);
            i = nal_end;
        }

        return (result, has_sps_pps);
    }

    // AVCC形式（NAL長プレフィックス）として処理
    debug!("MF encoder: detected AVCC format, converting to Annex-B");
    let mut i = 0;
    while i < data.len() {
        if i + 4 <= data.len() {
            // NAL長を読み取る（ビッグエンディアン）
            let nal_length =
                u32::from_be_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]) as usize;

            i += 4;

            if i + nal_length <= data.len() && nal_length > 0 {
                let nal_unit = &data[i..i + nal_length];

                // NALユニットのタイプを確認（SPS/PPS判定）
                if nal_unit.len() > 0 {
                    let nal_type = nal_unit[0] & 0x1F;
                    if nal_type == 7 || nal_type == 8 {
                        has_sps_pps = true;
                        debug!("MF encoder: found SPS/PPS in AVCC data (type={})", nal_type);
                    }
                }

                // スタートコードを追加
                result.extend_from_slice(START_CODE);
                result.extend_from_slice(nal_unit);

                i += nal_length;
            } else {
                // 無効なNAL長の場合は残りをコピーして終了
                if i < data.len() {
                    warn!("MF encoder: invalid NAL length, copying remaining data");
                    result.extend_from_slice(&data[i..]);
                }
                break;
            }
        } else {
            // データが不足している場合は残りをコピー
            if i < data.len() {
                result.extend_from_slice(&data[i..]);
            }
            break;
        }
    }

    (result, has_sps_pps)
}

/// AVCDecoderConfigurationRecord (avcC) を解析してSPS/PPSを抽出
/// フォーマット: ISO/IEC 14496-15 Annex E
pub fn parse_avc_decoder_config(data: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    if data.len() < 7 {
        return None;
    }

    // avcC構造:
    // [0] configurationVersion (1 byte) = 1
    // [1] AVCProfileIndication (1 byte)
    // [2] profile_compatibility (1 byte)
    // [3] AVCLevelIndication (1 byte)
    // [4] lengthSizeMinusOne (1 byte, lower 2 bits) - NAL長のバイト数 - 1
    // [5] numOfSequenceParameterSets (1 byte, lower 5 bits)
    // [6+] SPS/PPSデータ

    if data[0] != 1 {
        debug!("MF encoder: invalid configurationVersion in avcC");
        return None;
    }

    let num_sps = (data[5] & 0x1F) as usize;
    let mut offset = 6;

    // SPSを取得
    let mut sps: Option<Vec<u8>> = None;
    for i in 0..num_sps {
        if offset + 2 > data.len() {
            debug!("MF encoder: invalid SPS length in avcC");
            return None;
        }
        let sps_len = ((data[offset] as usize) << 8) | (data[offset + 1] as usize);
        offset += 2;

        if offset + sps_len > data.len() {
            debug!("MF encoder: SPS data out of bounds in avcC");
            return None;
        }

        if i == 0 {
            // 最初のSPSを使用
            sps = Some(data[offset..offset + sps_len].to_vec());
        }
        offset += sps_len;
    }

    // PPSを取得
    if offset >= data.len() {
        debug!("MF encoder: no PPS data in avcC");
        return None;
    }

    let num_pps = data[offset] as usize;
    offset += 1;

    let mut pps: Option<Vec<u8>> = None;
    for i in 0..num_pps {
        if offset + 2 > data.len() {
            debug!("MF encoder: invalid PPS length in avcC");
            return None;
        }
        let pps_len = ((data[offset] as usize) << 8) | (data[offset + 1] as usize);
        offset += 2;

        if offset + pps_len > data.len() {
            debug!("MF encoder: PPS data out of bounds in avcC");
            return None;
        }

        if i == 0 {
            // 最初のPPSを使用
            pps = Some(data[offset..offset + pps_len].to_vec());
        }
        offset += pps_len;
    }

    match (sps, pps) {
        (Some(s), Some(p)) => Some((s, p)),
        _ => {
            debug!("MF encoder: failed to extract both SPS and PPS from avcC");
            None
        }
    }
}
