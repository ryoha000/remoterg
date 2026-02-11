use anyhow::{Context, Result};
use tracing::{debug, warn};
use windows::core::Interface;
use windows::Win32::Media::MediaFoundation::{
    CODECAPI_AVEncCommonLowLatency, CODECAPI_AVEncMPVDefaultBPictureCount,
    CODECAPI_AVEncVideoForceKeyFrame, CODECAPI_AVLowLatencyMode, ICodecAPI, IMFMediaEventGenerator,
    IMFMediaType, IMFTransform, MFCreateMediaType, MFMediaType_Video, MFSampleExtension_CleanPoint,
    MFVideoFormat_H264, MFVideoFormat_NV12, MFVideoInterlace_Progressive,
    MFT_MESSAGE_COMMAND_FLUSH, MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,
    MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_SET_TYPE_TEST_ONLY, MF_E_INVALIDMEDIATYPE,
    MF_E_NO_MORE_TYPES, MF_LOW_LATENCY, MF_MT_MPEG_SEQUENCE_HEADER,
};

use crate::windows::codec::{EncodedFrame, HardwareEncoder};
use crate::windows::utils::d3d::D3D11Resources;

/// H.264データがAnnex-B形式（スタートコード）かどうかを判定
fn is_annexb_format(data: &[u8]) -> bool {
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
fn annexb_from_mf_data(data: &[u8]) -> (Vec<u8>, bool) {
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

/// 非同期ハードウェア H.264 エンコーダー
pub struct H264Encoder {
    transform: IMFTransform,
    event_generator: IMFMediaEventGenerator,
    #[allow(dead_code)]
    d3d_resources: D3D11Resources,
    width: u32,
    height: u32,
    first_keyframe_sent: bool,
}

impl H264Encoder {
    /// H.264 エンコーダーを作成
    pub fn create(d3d_resources: D3D11Resources, width: u32, height: u32) -> Result<Self> {
        unsafe {
            let transform = crate::windows::h264::utils::find_async_h264_encoder()
                .context("Failed to find async H.264 encoder MFT")?;

            // D3D マネージャーを設定
            d3d_resources.setup_mft(&transform)?;

            // IMFMediaEventGeneratorを取得（非同期MFTのイベント駆動に必要）
            let event_generator: IMFMediaEventGenerator = transform
                .cast()
                .ok()
                .context("Failed to get IMFMediaEventGenerator from transform")?;

            let mut encoder = Self {
                transform,
                event_generator,
                d3d_resources,
                width,
                height,
                first_keyframe_sent: false,
            };

            // 低遅延属性を設定（ベストエフォート、失敗しても無視）
            encoder.setup_low_latency_attributes()?;

            // メディアタイプを設定
            encoder
                .setup_media_types(width, height)
                .with_context(|| format!("Failed to setup media types for {}x{}", width, height))
                .map_err(|e| {
                    tracing::error!("Media type setup failed: {:?}", e);
                    e
                })?;

            Ok(encoder)
        }
    }

    /// サポートされている入力解像度を検出
    fn detect_supported_resolutions(&self) -> Result<Vec<(u32, u32)>> {
        unsafe {
            let mut supported_resolutions = Vec::new();
            let mut type_index = 0u32;

            loop {
                match self.transform.GetInputAvailableType(0, type_index) {
                    Ok(mt) => {
                        // メジャータイプを確認
                        let major_type = mt
                            .GetGUID(&windows::Win32::Media::MediaFoundation::MF_MT_MAJOR_TYPE)
                            .ok()
                            .context(format!(
                                "Failed to get input major type at index {}",
                                type_index
                            ))?;

                        if major_type == MFMediaType_Video {
                            // サブタイプを確認
                            let subtype = mt
                                .GetGUID(&windows::Win32::Media::MediaFoundation::MF_MT_SUBTYPE)
                                .ok()
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
    fn setup_media_types(&mut self, width: u32, height: u32) -> Result<()> {
        unsafe {
            let frame_size = ((width as u64) << 32) | (height as u64);
            let frame_rate = (60u64 << 32) | 1u64;

            // 非同期MFTでは、出力メディアタイプを先に設定してから、
            // 入力メディアタイプを設定する必要がある
            // これにより、エンコーダーが出力形式を認識してから入力形式を受け入れることができる

            debug!("Setting output media type first");

            // 出力メディアタイプを列挙してH.264形式を探す
            debug!("Enumerating output media types for H.264 encoder");
            let mut output_media_type: Option<IMFMediaType> = None;
            let mut type_index = 0u32;

            loop {
                match self.transform.GetOutputAvailableType(0, type_index) {
                    Ok(mt) => {
                        // メジャータイプを確認
                        let major_type = mt
                            .GetGUID(&windows::Win32::Media::MediaFoundation::MF_MT_MAJOR_TYPE)
                            .ok()
                            .context(format!(
                                "Failed to get output major type at index {}",
                                type_index
                            ))?;

                        if major_type == MFMediaType_Video {
                            // サブタイプを確認
                            let subtype = mt
                                .GetGUID(&windows::Win32::Media::MediaFoundation::MF_MT_SUBTYPE)
                                .ok()
                                .context(format!(
                                    "Failed to get output subtype at index {}",
                                    type_index
                                ))?;

                            debug!(
                                "Found output media type at index {}: major={:?}, subtype={:?}",
                                type_index, major_type, subtype
                            );

                            if subtype == MFVideoFormat_H264 {
                                debug!("Found H.264 output media type at index {}", type_index);
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
                    "No H.264 output media type found after enumerating {} types",
                    type_index
                )
            })?;

            // 列挙されたメディアタイプをコピーして新しいメディアタイプを作成
            let configured_output_type = MFCreateMediaType()
                .ok()
                .context("Failed to create output media type for configuration")?;

            // 列挙されたメディアタイプからすべての属性をコピー
            output_media_type
                .CopyAllItems(&configured_output_type)
                .ok()
                .context("Failed to copy output media type attributes")?;

            // 必要な属性を設定
            configured_output_type
                .SetUINT64(
                    &windows::Win32::Media::MediaFoundation::MF_MT_FRAME_SIZE,
                    frame_size,
                )
                .ok()
                .context("Failed to set output frame size")?;

            configured_output_type
                .SetUINT64(
                    &windows::Win32::Media::MediaFoundation::MF_MT_FRAME_RATE,
                    frame_rate,
                )
                .ok()
                .context("Failed to set output frame rate")?;

            configured_output_type
                .SetUINT32(
                    &windows::Win32::Media::MediaFoundation::MF_MT_INTERLACE_MODE,
                    MFVideoInterlace_Progressive.0 as u32,
                )
                .ok()
                .context("Failed to set output interlace mode")?;

            // 出力メディアタイプを設定
            self.transform
                .SetOutputType(0, &configured_output_type, 0)
                .map_err(|e| {
                    anyhow::anyhow!(
                        "Failed to set H.264 encoder output type (width={}, height={}): {}",
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
                        let result = self.transform.GetInputAvailableType(0, count);
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
                        let test_result = self.transform.SetInputType(
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
                        self.transform.SetInputType(0, &configured_input_type, 0)?;
                        break Ok(Some(configured_input_type));
                    }
                })()
                .map_err(|e| {
                    // サポートされている解像度を検出してエラーメッセージに含める
                    let supported_resolutions =
                        self.detect_supported_resolutions().unwrap_or_default();

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
                            resolutions_str.push_str(&format!(
                                ", ... ({} total)",
                                supported_resolutions.len()
                            ));
                        }
                        format!("Supported resolutions include: {}", resolutions_str)
                    };

                    anyhow::anyhow!(
                        "Failed to set H.264 encoder input type (width={}, height={}): {}. {}",
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
    fn setup_low_latency_attributes(&self) -> Result<()> {
        unsafe {
            // Attributes を取得
            let attributes = self.transform.GetAttributes()?;
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

            Ok(())
        }
    }

    /// 出力メディアタイプからcodec config (SPS/PPS) を取得（best-effort）
    /// 戻り値: (SPS NAL, PPS NAL) - 取得できない場合はNone
    fn get_codec_config_internal(&self) -> Option<(Vec<u8>, Vec<u8>)> {
        unsafe {
            // 出力CurrentTypeを取得
            let output_type = match self.transform.GetOutputCurrentType(0) {
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
}

impl HardwareEncoder for H264Encoder {
    fn transform(&self) -> &IMFTransform {
        &self.transform
    }

    fn event_generator(&self) -> &IMFMediaEventGenerator {
        &self.event_generator
    }

    fn start_streaming(&self) -> Result<()> {
        unsafe {
            self.transform
                .ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0)
                .ok()
                .context("Failed to flush encoder")?;

            self.transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
                .ok()
                .context("Failed to notify begin streaming")?;

            self.transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
                .ok()
                .context("Failed to notify start of stream")?;

            Ok(())
        }
    }

    fn resize(&mut self, width: u32, height: u32) -> Result<()> {
        if self.width != width || self.height != height {
            self.width = width;
            self.height = height;
            self.setup_media_types(width, height)
                .context("Failed to resize H.264 encoder")?;
        }
        Ok(())
    }

    fn set_force_keyframe(&self, force: bool) -> Result<()> {
        unsafe {
            let codec_api: ICodecAPI = self
                .transform
                .cast()
                .ok()
                .context("Failed to cast transform to ICodecAPI")?;
            // CODECAPI_AVEncVideoForceKeyFrameを設定（値は1）
            codec_api
                .SetValue(&CODECAPI_AVEncVideoForceKeyFrame, &force.into())
                .map_err(|e| {
                    anyhow::anyhow!("Failed to set CODECAPI_AVEncVideoForceKeyFrame: {}", e)
                })?;
            Ok(())
        }
    }

    fn get_codec_config(&self) -> Option<Vec<u8>> {
        // H.264の場合、SPS/PPSをAnnex-B形式で返すこともできるが、
        // 外部に返す必要性が低いため、ここではNoneを返しておくか、
        // もしくは実装するか。
        // 現在の要件ではprocess_output内で処理が完結するため、Noneで良い。
        None
    }

    fn process_output(
        &mut self,
        sample: &windows::Win32::Media::MediaFoundation::IMFSample,
    ) -> Result<EncodedFrame> {
        unsafe {
            let buffer = sample
                .GetBufferByIndex(0)
                .context("Failed to get output buffer")?;

            let mut data_ptr: *mut u8 = std::ptr::null_mut();
            let mut max_length: u32 = 0;
            buffer
                .Lock(&mut data_ptr, Some(&mut max_length), None)
                .context("Failed to lock output buffer")?;

            let current_length = match buffer.GetCurrentLength() {
                Ok(len) => len,
                Err(e) => {
                    let _ = buffer.Unlock();
                    return Err(anyhow::anyhow!("Failed to get output buffer length: {}", e));
                }
            };

            let mut encoded_data = Vec::new();
            if current_length > 0 && !data_ptr.is_null() {
                let slice = std::slice::from_raw_parts(data_ptr, current_length as usize);
                encoded_data.extend_from_slice(slice);
            }

            if let Err(e) = buffer.Unlock() {
                warn!("MF encoder: failed to unlock output buffer: {}", e);
            }

            // Annex-B形式に変換（フォーマット自動判定）
            let (mut sample_data, has_sps_pps_in_data) = annexb_from_mf_data(&encoded_data);

            // キーフレーム判定（MFSampleExtension_CleanPoint + SPS/PPS検出）
            let is_clean_point = match sample.GetUINT32(&MFSampleExtension_CleanPoint) {
                Ok(1) => true,
                Ok(0) => false,
                _ => false, // エラーまたは未設定の場合はfalse
            };
            // SPS/PPSが含まれている場合もキーフレームとして扱う（ブラウザがデコード開始できるように）
            let mut is_keyframe = is_clean_point || has_sps_pps_in_data;

            // in-bandにSPS/PPSが無く、codec configから取得したSPS/PPSがある場合、最初のキーフレームに注入
            if !has_sps_pps_in_data && is_keyframe && !self.first_keyframe_sent {
                // ここでSPS/PPSを取得
                if let Some((ref sps, ref pps)) = self.get_codec_config_internal() {
                    debug!(
                        "MF encoder: injecting SPS/PPS from codec config (SPS: {} bytes, PPS: {} bytes)",
                        sps.len(),
                        pps.len()
                    );
                    const START_CODE: &[u8] = &[0x00, 0x00, 0x00, 0x01];
                    let mut injected_data = Vec::with_capacity(
                        START_CODE.len()
                            + sps.len()
                            + START_CODE.len()
                            + pps.len()
                            + sample_data.len(),
                    );
                    injected_data.extend_from_slice(START_CODE);
                    injected_data.extend_from_slice(sps.as_slice());
                    injected_data.extend_from_slice(START_CODE);
                    injected_data.extend_from_slice(pps.as_slice());
                    injected_data.extend_from_slice(&sample_data);
                    sample_data = injected_data;
                    is_keyframe = true; // 注入後は確実にキーフレーム
                    self.first_keyframe_sent = true;
                }
            } else if has_sps_pps_in_data {
                debug!("MF encoder: detected SPS/PPS in encoded data, marking as keyframe");
                self.first_keyframe_sent = true;
            }

            Ok(EncodedFrame {
                data: sample_data,
                is_keyframe,
            })
        }
    }
}

/// AVCDecoderConfigurationRecord (avcC) を解析してSPS/PPSを抽出
/// フォーマット: ISO/IEC 14496-15 Annex E
fn parse_avc_decoder_config(data: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
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

// Media FoundationのCOMオブジェクトは一般的にスレッドセーフ（特に非同期MFT）
unsafe impl Send for H264Encoder {}
