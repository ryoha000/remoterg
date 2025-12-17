use anyhow::{Context, Result};
use tracing::debug;
use windows::core::Interface;
use windows::Win32::Media::MediaFoundation::{
    CODECAPI_AVEncCommonLowLatency, CODECAPI_AVEncMPVDefaultBPictureCount,
    CODECAPI_AVEncVideoForceKeyFrame, CODECAPI_AVLowLatencyMode, ICodecAPI, IMFMediaEventGenerator,
    IMFMediaType, IMFTransform, MFCreateMediaType, MFMediaType_Video, MFVideoFormat_H264,
    MFVideoFormat_NV12, MFVideoInterlace_Progressive, MFT_MESSAGE_COMMAND_FLUSH,
    MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_SET_TYPE_TEST_ONLY,
    MF_E_INVALIDMEDIATYPE, MF_E_NO_MORE_TYPES, MF_LOW_LATENCY, MF_MT_MPEG_SEQUENCE_HEADER,
};

use crate::h264::mmf::d3d::D3D11Resources;

/// 非同期ハードウェア H.264 エンコーダー
pub struct H264Encoder {
    transform: IMFTransform,
    event_generator: IMFMediaEventGenerator,
    d3d_resources: D3D11Resources,
    width: u32,
    height: u32,
}

impl H264Encoder {
    /// H.264 エンコーダーを作成
    pub fn create(d3d_resources: D3D11Resources, width: u32, height: u32) -> Result<Self> {
        unsafe {
            let transform = crate::h264::mmf::mf::find_async_h264_encoder()
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
                    Err(e) if e.code() == MF_E_NO_MORE_TYPES => {
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
                    Err(e) if e.code() == MF_E_NO_MORE_TYPES => {
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
                            Err(error) if error.code() == MF_E_NO_MORE_TYPES => {
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
                            Err(error) if error.code() == MF_E_INVALIDMEDIATYPE => {
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

    /// 解像度が変更された場合に再設定
    pub fn resize(&mut self, width: u32, height: u32) -> Result<()> {
        if self.width != width || self.height != height {
            self.width = width;
            self.height = height;
            self.setup_media_types(width, height)
                .context("Failed to resize H.264 encoder")?;
        }
        Ok(())
    }

    /// transform への参照を取得（イベントループから使用）
    pub fn transform(&self) -> &IMFTransform {
        &self.transform
    }

    /// event_generator への参照を取得（イベントループから使用）
    pub fn event_generator(&self) -> &IMFMediaEventGenerator {
        &self.event_generator
    }

    /// ストリーミングを開始（参考実装に従い、Flush → BeginStreaming → StartOfStream）
    pub fn start_streaming(&self) -> Result<()> {
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

    /// 次のフレームをキーフレームとして強制
    pub fn set_force_keyframe(&self, force: bool) -> Result<()> {
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

    /// 出力メディアタイプからcodec config (SPS/PPS) を取得（best-effort）
    /// 戻り値: (SPS NAL, PPS NAL) - 取得できない場合はNone
    pub fn get_codec_config(&self) -> Option<(Vec<u8>, Vec<u8>)> {
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
