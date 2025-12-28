use anyhow::{Context, Result};
use core_types::{AudioCaptureCommandReceiver, AudioCaptureMessage, AudioFrame, AudioFrameSender};
use std::ptr;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;
use tracing::{debug, error, info};
use windows::core::HRESULT;
use windows::core::{implement, Interface, Ref};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows::Win32::Media::Audio::{
    ActivateAudioInterfaceAsync, IActivateAudioInterfaceAsyncOperation,
    IActivateAudioInterfaceCompletionHandler, IActivateAudioInterfaceCompletionHandler_Impl,
    IAudioCaptureClient, IAudioClient, AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED,
    AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_LOOPBACK,
    AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, AUDIOCLIENT_ACTIVATION_PARAMS,
    AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
    PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE, VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
    WAVEFORMATEX,
};
use windows::Win32::Media::Multimedia::WAVE_FORMAT_IEEE_FLOAT;
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::System::Threading::{CreateEventW, SetEvent, WaitForSingleObject, INFINITE};
use windows::Win32::System::Variant::VT_BLOB;
use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

/// 音声キャプチャサービス
pub struct AudioCaptureService {
    frame_tx: AudioFrameSender,
    command_rx: AudioCaptureCommandReceiver,
}

impl AudioCaptureService {
    pub fn new(frame_tx: AudioFrameSender, command_rx: AudioCaptureCommandReceiver) -> Self {
        Self {
            frame_tx,
            command_rx,
        }
    }

    pub async fn run(mut self) -> Result<()> {
        info!("AudioCaptureService started");

        let mut capture_task: Option<(std::thread::JoinHandle<Result<()>>, Arc<AtomicBool>)> = None;

        loop {
            tokio::select! {
                msg = self.command_rx.recv() => {
                    match msg {
                        Some(AudioCaptureMessage::Start { hwnd }) => {
                            info!("Start audio capture for HWND: {hwnd}");

                            // 既存のキャプチャタスクを停止
                            if let Some((handle, stop_flag)) = capture_task.take() {
                                stop_flag.store(true, Ordering::Relaxed);
                                let _ = handle.join();
                            }

                            // 新しいキャプチャタスクを開始
                            let frame_tx = self.frame_tx.clone();
                            let stop_flag = Arc::new(AtomicBool::new(false));
                            let stop_flag_clone = stop_flag.clone();
                            let handle = thread::spawn(move || {
                                Self::capture_loop(hwnd, frame_tx, stop_flag_clone)
                            });
                            capture_task = Some((handle, stop_flag));
                        }
                        Some(AudioCaptureMessage::Stop) => {
                            info!("Stop audio capture");
                            if let Some((handle, stop_flag)) = capture_task.take() {
                                stop_flag.store(true, Ordering::Relaxed);
                                let _ = handle.join();
                            }
                        }
                        None => {
                            debug!("Audio capture command channel closed");
                            break;
                        }
                    }
                }
            }
        }

        // クリーンアップ
        if let Some((handle, stop_flag)) = capture_task.take() {
            stop_flag.store(true, Ordering::Relaxed);
            let _ = handle.join();
        }

        info!("AudioCaptureService stopped");
        Ok(())
    }

    fn capture_loop(
        hwnd: u64,
        frame_tx: AudioFrameSender,
        stop_flag: Arc<AtomicBool>,
    ) -> Result<()> {
        // HWNDからプロセスIDを取得
        let mut process_id: u32 = 0;
        unsafe {
            GetWindowThreadProcessId(HWND(hwnd as *mut _), Some(&mut process_id));
        }
        if process_id == 0 {
            return Err(anyhow::anyhow!("Failed to get process ID from HWND"));
        }
        info!("Process ID: {}", process_id);

        // COMを初期化
        unsafe {
            let coinit_result = CoInitializeEx(None, COINIT_MULTITHREADED);
            if coinit_result.is_err() {
                if coinit_result != windows::core::HRESULT(0x800401F0u32 as i32) {
                    // CO_E_ALREADYINITIALIZED
                    return Err(anyhow::anyhow!(
                        "Failed to initialize COM: {:?}",
                        coinit_result
                    ));
                }
                info!("COM already initialized");
            }
        }

        // QPC周波数を取得
        let mut qpc_freq: i64 = 0;
        unsafe {
            if QueryPerformanceFrequency(&mut qpc_freq).is_err() {
                return Err(anyhow::anyhow!("Failed to get QPC frequency"));
            }
        }
        if qpc_freq <= 0 {
            return Err(anyhow::anyhow!("Invalid QPC frequency: {}", qpc_freq));
        }
        let ticks_to_hns = 10_000_000.0 / qpc_freq as f64;
        info!(
            "QPC frequency: {}, ticks_to_hns: {}",
            qpc_freq, ticks_to_hns
        );

        // フォーマットを設定（48kHz, ステレオ, 32-bit float）
        let wave_format = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_IEEE_FLOAT as u16,
            nChannels: 2,
            nSamplesPerSec: 48000,
            nAvgBytesPerSec: 48000 * 2 * 4, // サンプルレート * チャンネル数 * 4バイト(float)
            nBlockAlign: 2 * 4,             // チャンネル数 * 4バイト
            wBitsPerSample: 32,
            cbSize: 0,
        };

        // ActivateAudioInterfaceAsyncを使用してプロセスループバックモードでオーディオクライアントを取得
        let audio_client = unsafe {
            Self::setup_audio_client(process_id, &wave_format)
                .context("Failed to setup audio client")?
        };

        // キャプチャクライアントを取得
        let capture_client = unsafe {
            audio_client
                .GetService::<IAudioCaptureClient>()
                .context("Failed to get capture client")?
        };

        // キャプチャを開始
        unsafe {
            audio_client
                .Start()
                .context("Failed to start audio capture")?;
        }
        info!("Audio capture started");

        // 初期QPC値を取得
        let mut start_qpc: i64 = 0;
        unsafe {
            if QueryPerformanceCounter(&mut start_qpc).is_err() {
                return Err(anyhow::anyhow!("Failed to get initial QPC value"));
            }
        }
        let start_qpc = start_qpc as u64;
        info!("Initial QPC value: {}", start_qpc);

        // 10msフレームサイズ（480サンプル @ 48kHz）
        const FRAME_SIZE_SAMPLES: u32 = 480;

        let mut accumulated_samples: Vec<f32> = Vec::new();
        let mut last_packet_qpc: u64 = start_qpc;

        loop {
            if stop_flag.load(Ordering::Relaxed) {
                return Ok(());
            }
            // GetNextPacketSizeでパケットサイズを確認
            let next_packet_size = unsafe {
                capture_client
                    .GetNextPacketSize()
                    .context("Failed to get next packet size")?
            };

            if next_packet_size == 0 {
                thread::sleep(Duration::from_millis(1));
                if stop_flag.load(Ordering::Relaxed) {
                    return Ok(());
                }
                continue;
            }

            // バッファを取得
            let mut buffer = ptr::null_mut();
            let mut num_frames_available = 0u32;
            let mut flags = 0u32;
            let mut device_position = 0u64;
            let mut qpc_position: u64 = 0;

            unsafe {
                capture_client
                    .GetBuffer(
                        &mut buffer,
                        &mut num_frames_available,
                        &mut flags,
                        Some(&mut device_position),
                        Some(&mut qpc_position as *mut u64),
                    )
                    .context("Failed to get buffer")?;
            }

            // QPCタイミングの検証
            if qpc_position <= last_packet_qpc {
                info!(
                    "QPC time went backwards: current={}, last={}",
                    qpc_position, last_packet_qpc
                );
            }
            last_packet_qpc = qpc_position;

            // サイレントフラグをチェックしてデータを処理
            // bufferをスコープから外すために、データを先にコピー
            let frames_to_process = if (flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32)) == 0
                && !buffer.is_null()
                && num_frames_available > 0
            {
                // float配列として読み取ってコピー
                let data_slice = unsafe {
                    std::slice::from_raw_parts(
                        buffer as *const f32,
                        (num_frames_available * 2) as usize, // ステレオなので2倍
                    )
                };
                Some(data_slice.to_vec())
            } else {
                None
            };

            // bufferをスコープから外す（ReleaseBufferはnum_frames_availableのみ必要）
            let frames_count = num_frames_available;
            let _ = buffer;

            // コピーしたデータを処理
            if let Some(data) = frames_to_process {
                // サンプルを蓄積
                accumulated_samples.extend_from_slice(&data);

                // 10msフレーム（480サンプル）分がたまったら送信
                while accumulated_samples.len() >= FRAME_SIZE_SAMPLES as usize * 2 {
                    let frame_samples: Vec<f32> = accumulated_samples
                        .drain(..(FRAME_SIZE_SAMPLES as usize * 2))
                        .collect();

                    // QPCを使用してタイムスタンプを計算
                    let relative_qpc = qpc_position.saturating_sub(start_qpc);
                    let time_hns = (relative_qpc as f64 * ticks_to_hns) as i64;
                    let timestamp_us = (time_hns / 10) as u64; // 100ナノ秒からマイクロ秒へ変換

                    let audio_frame = AudioFrame {
                        samples: frame_samples,
                        sample_rate: 48000,
                        channels: 2,
                        timestamp_us,
                    };

                    if let Err(e) = frame_tx.blocking_send(audio_frame) {
                        error!("Failed to send audio frame: {}", e);
                        return Err(anyhow::anyhow!("Failed to send audio frame: {}", e));
                    }

                    debug!(
                        "Sent audio frame: {} samples, timestamp: {}us",
                        FRAME_SIZE_SAMPLES * 2,
                        timestamp_us
                    );
                }
            }

            // バッファを解放
            unsafe {
                capture_client
                    .ReleaseBuffer(frames_count)
                    .context("Failed to release buffer")?;
            }
        }
    }

    unsafe fn setup_audio_client(
        process_id: u32,
        wave_format: &WAVEFORMATEX,
    ) -> Result<IAudioClient> {
        info!("Setting up audio client for process ID: {}", process_id);

        // AUDIOCLIENT_ACTIVATION_PARAMSを作成
        let mut activation_params = AUDIOCLIENT_ACTIVATION_PARAMS::default();
        activation_params.ActivationType = AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK;
        activation_params
            .Anonymous
            .ProcessLoopbackParams
            .ProcessLoopbackMode = PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE;
        activation_params
            .Anonymous
            .ProcessLoopbackParams
            .TargetProcessId = process_id;

        // PROPVARIANTを構築（VT_BLOBとして）
        let mut prop_variant = PROPVARIANT::default();
        (*prop_variant.Anonymous.Anonymous).vt = VT_BLOB;
        (*prop_variant.Anonymous.Anonymous).Anonymous.blob.cbSize =
            std::mem::size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32;
        (*prop_variant.Anonymous.Anonymous).Anonymous.blob.pBlobData =
            &activation_params as *const _ as *mut u8;

        // Windows Event を作成
        let ev = CreateEventW(None, false, false, None).context("Failed to create event")?;

        // コールバックハンドラを作成
        let handler: IActivateAudioInterfaceCompletionHandler = SyncActivationHandler(ev).into();

        // ActivateAudioInterfaceAsyncを呼び出し
        let activate_operation = ActivateAudioInterfaceAsync(
            VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
            &IAudioClient::IID,
            Some(&prop_variant),
            &handler,
        )
        .context("Failed to activate audio interface")?;

        // イベントがシグナルされるまで待機
        WaitForSingleObject(ev, INFINITE);

        // 結果を取得
        let mut hr = HRESULT(0);
        let mut audio_interface: Option<windows::core::IUnknown> = None;
        activate_operation
            .GetActivateResult(&mut hr, &mut audio_interface)
            .context("Failed to get activate result")?;

        // Event を閉じる
        CloseHandle(ev).context("Failed to close event")?;

        // PROPVARIANT のライフタイム管理（activation_params への参照を含むため）
        std::mem::forget(prop_variant);

        // IAudioClient にキャスト
        let audio_client = audio_interface
            .ok_or_else(|| anyhow::anyhow!("Audio interface is None"))?
            .cast::<IAudioClient>()
            .map_err(|e| anyhow::anyhow!("Failed to cast to IAudioClient: {:?}", e))?;

        // オーディオクライアントを初期化
        audio_client
            .Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK
                    | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
                    | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
                10_000_000, // 100msバッファ
                0,
                wave_format,
                None,
            )
            .context("Failed to initialize audio client")?;

        Ok(audio_client)
    }
}

/// ActivateAudioInterfaceAsyncのコールバックハンドラ
#[implement(IActivateAudioInterfaceCompletionHandler)]
struct SyncActivationHandler(HANDLE);

impl IActivateAudioInterfaceCompletionHandler_Impl for SyncActivationHandler_Impl {
    fn ActivateCompleted(
        &self,
        _: Ref<'_, IActivateAudioInterfaceAsyncOperation>,
    ) -> windows::core::Result<()> {
        unsafe { SetEvent(self.0) }
    }
}
