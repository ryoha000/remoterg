use openh264::encoder::EncodedBitStream;
use tracing::{debug, info, warn};

/// OpenH264のEncodedBitStreamからAnnex-B形式のH.264データを生成
/// 戻り値: (Annex-B形式のデータ, SPS/PPSが含まれているか)
pub fn annexb_from_bitstream(bitstream: &EncodedBitStream) -> (Vec<u8>, bool) {
    const START_CODE: &[u8] = &[0x00, 0x00, 0x00, 0x01];
    const START_CODE_SIZE: usize = 4;
    let mut has_sps_pps = false;

    let num_layers = bitstream.num_layers();
    if num_layers == 0 {
        warn!("EncodedBitStream has no layers");
        return (Vec::new(), has_sps_pps);
    }

    debug!("Processing {} layers", num_layers);

    // まず総サイズを推定してreserve（2パス化は避ける）
    let mut estimated_size = 0usize;
    for i in 0..num_layers {
        if let Some(layer) = bitstream.layer(i) {
            let nal_count = layer.nal_count();
            for j in 0..nal_count {
                if let Some(nal_unit) = layer.nal_unit(j) {
                    if !nal_unit.is_empty() {
                        let has_start_code = nal_unit.len() >= 4
                            && nal_unit[0] == 0x00
                            && nal_unit[1] == 0x00
                            && nal_unit[2] == 0x00
                            && nal_unit[3] == 0x01;
                        estimated_size += nal_unit.len();
                        if !has_start_code {
                            estimated_size += START_CODE_SIZE;
                        }
                    }
                }
            }
        }
    }

    let mut sample_data = Vec::with_capacity(estimated_size);

    // 実際のデータを構築
    for i in 0..num_layers {
        if let Some(layer) = bitstream.layer(i) {
            let nal_count = layer.nal_count();
            debug!("Layer {}: {} NAL units", i, nal_count);

            if nal_count == 0 {
                warn!("Layer {} has no NAL units", i);
                continue;
            }

            for j in 0..nal_count {
                if let Some(nal_unit) = layer.nal_unit(j) {
                    if nal_unit.is_empty() {
                        warn!("NAL unit {} in layer {} is empty", j, i);
                        continue;
                    }

                    let has_start_code = nal_unit.len() >= 4
                        && nal_unit[0] == 0x00
                        && nal_unit[1] == 0x00
                        && nal_unit[2] == 0x00
                        && nal_unit[3] == 0x01;

                    let nal_header_offset = if has_start_code { 4 } else { 0 };

                    if nal_unit.len() <= nal_header_offset {
                        warn!(
                            "NAL unit {} in layer {} is too small ({} bytes, offset {})",
                            j,
                            i,
                            nal_unit.len(),
                            nal_header_offset
                        );
                        continue;
                    }

                    let nal_type = nal_unit[nal_header_offset] & 0x1F;
                    if nal_type == 7 || nal_type == 8 {
                        has_sps_pps = true;
                        info!(
                            "Found SPS/PPS: type={}, size={} bytes",
                            nal_type,
                            nal_unit.len()
                        );
                    }

                    if !has_start_code {
                        sample_data.extend_from_slice(START_CODE);
                    }

                    sample_data.extend_from_slice(nal_unit);
                } else {
                    warn!("NAL unit {} in layer {} is None", j, i);
                }
            }
        } else {
            warn!("Layer {} is None", i);
        }
    }

    debug!(
        "Total sample data: {} bytes (estimated: {}), has_sps_pps: {}",
        sample_data.len(),
        estimated_size,
        has_sps_pps
    );

    (sample_data, has_sps_pps)
}


