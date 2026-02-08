use super::factory::MediaFoundationAV1EncoderFactory;
use core_types::{EncodeJob, VideoEncoderFactory};
use std::fs::File;
use std::io::Write;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[tokio::test]
async fn test_av1_dummy_encode() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .try_init()
        .ok();

    // 1. Setup Encoder
    let factory = MediaFoundationAV1EncoderFactory::new();
    let (job_slot, mut result_rx) = factory.setup();

    let width: u32 = 1920;
    let height: u32 = 1080;
    let fps: u32 = 60;
    let duration_secs: u32 = 10;
    let total_frames = fps * duration_secs; // 600 frames

    // 2. Prepare IVF Output File
    let mut file = File::create("test_output.ivf")?;

    // IVF Header (Placeholder)
    file.write_all(b"DKIF")?; // signature
    file.write_all(&0u16.to_le_bytes())?; // version
    file.write_all(&32u16.to_le_bytes())?; // header length
    file.write_all(b"AV01")?; // codec
    file.write_all(&(width as u16).to_le_bytes())?; // width
    file.write_all(&(height as u16).to_le_bytes())?; // height
    file.write_all(&fps.to_le_bytes())?; // rate
    file.write_all(&1u32.to_le_bytes())?; // scale
    file.write_all(&0u32.to_le_bytes())?; // length (Placeholder)
    file.write_all(&0u32.to_le_bytes())?; // unused

    // 3. Encode Loop
    let start_time = Instant::now();
    
    // Spawn a task to feed frames
    tokio::spawn(async move {
        for i in 0..total_frames {
            let timestamp = (i as u64 * 10_000_000) / fps as u64; // 100ns units
            
            // Dummy Frame Generation (Moving square)
            let mut rgba = vec![0u8; (width * height * 4) as usize];
            
            let box_size = 100;
            let speed = 5;
            let x = (i * speed) % (width - box_size);
            let y = (i * speed / 2) % (height - box_size);

            for by in 0..box_size {
                for bx in 0..box_size {
                    let idx = ((y + by) * width + (x + bx)) as usize * 4;
                    rgba[idx] = 255;     // R
                    rgba[idx + 1] = 0;   // G
                    rgba[idx + 2] = 0;   // B
                    rgba[idx + 3] = 255; // A
                }
            }

            let job = EncodeJob {
                width: width,
                height: height,
                rgba: Arc::new(rgba),
                timestamp: timestamp,
                enqueue_at: Instant::now(),
                request_keyframe: i % 60 == 0,
                frame_id: i as u64,
            };

            job_slot.set(job);
            
            // Wait to emulate 60fps feeding
            tokio::time::sleep(Duration::from_micros(16666)).await;
        }
    });

    // Receive Loop
    let mut frames_received = 0u32;
    let mut max_frame_id = 0u64;
    
    // We expect frames up to (total_frames - 1).
    // Due to drops, we might miss the last one.
    // We also need a timeout relative to duration.
    let timeout_duration = Duration::from_secs(duration_secs as u64 + 5);

    loop {
        // Use timeout for recv
        let result = tokio::select! {
             res = result_rx.recv() => res,
             _ = tokio::time::sleep(Duration::from_secs(2)) => {
                 println!("No frames received for 2 seconds. Finising.");
                 break;
             }
        };

        if let Some(result) = result {
            // println!("Received frame {} ({} bytes, keyframe: {})", result.frame_id, result.sample_data.len(), result.is_keyframe);
            
            // Write IVF Frame Header
            let len = result.sample_data.len() as u32;
            file.write_all(&len.to_le_bytes())?;
            file.write_all(&result.frame_id.to_le_bytes())?; // timestamp (use frame_id as pts for simplicity?) 
            // Ideally should use result.timestamp? EncodeResult doesn't have timestamp, 
            // but it has frame_id which correlates to input timestamp.
            // For IVF, timestamp is 64-bit unit.
            
            file.write_all(&result.sample_data)?;

            frames_received += 1;
            max_frame_id = result.frame_id;
            
            if max_frame_id >= (total_frames - 1) as u64 {
                println!("Received last frame. Finishing.");
                break;
            }
        } else {
            // Channel closed
            break;
        }

        if start_time.elapsed() > timeout_duration {
            println!("Test timed out.");
            break;
        }
    }

    println!("Encoded {} frames. Max frame ID: {}", frames_received, max_frame_id);

    // Update Header with actual frame count
    use std::io::Seek;
    file.seek(std::io::SeekFrom::Start(24))?;
    file.write_all(&frames_received.to_le_bytes())?;

    Ok(())
}

#[tokio::test]
async fn test_enumerate_encoders() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .try_init()
        .ok();

    use windows::Win32::Media::MediaFoundation::{
        MFTEnumEx, MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG_ALL, MFT_REGISTER_TYPE_INFO, MFMediaType_Video,
        MFVideoFormat_H264, MFVideoFormat_NV12,
    };
    use windows::core::{GUID, PWSTR};
    
    // Manual definition of AV1 GUID if not available
    #[allow(non_snake_case)]
    let MFVideoFormat_AV1 = GUID::from_u128(0x203326d0_2081_438f_9c03_b130e89ca7fa);

    unsafe {
        crate::windows::utils::mf::init_media_foundation();

        let mut data = std::ptr::null_mut();
        let mut len = 0;
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_ENCODER,
            MFT_ENUM_FLAG_ALL,
            None,
            None,
            &mut data,
            &mut len,
        )?;

        // Need to clean up data using CoTaskMemFree? 
        // Array::from_raw_parts handles ownership if we don't clone?
        // Wait, MFTEnumEx returns separate array of pointers?
        // According to windows::core::Array doc, it takes ownership.
        // Actually, MFTEnumEx returns pointer to array of IMFActivate pointers.
        
        let activates = windows::core::Array::<windows::Win32::Media::MediaFoundation::IMFActivate>::from_raw_parts(data as _, len);
        
        println!("Found {} video encoders:", activates.len());
        
        for (i, activate) in activates.as_slice().iter().enumerate() {
            if let Some(activate) = activate {
                let mut name_ptr: PWSTR = PWSTR::null();
                let mut name_len = 0;
                let _ = activate.GetAllocatedString(
                    &windows::Win32::Media::MediaFoundation::MFT_FRIENDLY_NAME_Attribute,
                    &mut name_ptr,
                    &mut name_len,
                );
                let name = if !name_ptr.is_null() {
                    name_ptr.to_string().unwrap_or_default()
                } else {
                    "Unknown".to_string()
                };
                
                println!("    {}: {}", i, name);

                if name.contains("AV1") {
                    println!("    -> Inspecting AV1 Encoder");
                     // Activate
                     let transform: windows::Win32::Media::MediaFoundation::IMFTransform = activate.ActivateObject().unwrap();
                     
                     // Check Attributes for Async
                     let attributes = transform.GetAttributes().unwrap();
                     let is_async = attributes.GetUINT32(&windows::Win32::Media::MediaFoundation::MF_TRANSFORM_ASYNC).unwrap_or(0);
                     println!("       Async Attribute: {}", is_async);

                     // Check Input Types
                     println!("       Available Input Types:");
                     let mut type_index = 0;
                     while let Ok(media_type) = transform.GetInputAvailableType(0, type_index) {
                         let subtype = media_type.GetGUID(&windows::Win32::Media::MediaFoundation::MF_MT_SUBTYPE).unwrap();
                         println!("         Input #{}: {:?}", type_index, subtype);
                         type_index += 1;
                     }
                     
                     // Check Output Types
                     println!("       Available Output Types:");
                     let mut type_index = 0;
                     while let Ok(media_type) = transform.GetOutputAvailableType(0, type_index) {
                        let subtype = media_type.GetGUID(&windows::Win32::Media::MediaFoundation::MF_MT_SUBTYPE).unwrap();
                        println!("         Output #{}: {:?}", type_index, subtype);
                        type_index += 1;
                    }
                }
            }
        }

        println!("--------------------------------------------------");
        println!("Testing Strict Search (HARDWARE | ASYNCMFT):");

        let input_type = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: MFVideoFormat_NV12,
        };

        let output_type = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: MFVideoFormat_AV1,
        };
        
        // Try search with Flags only
        println!("Testing Search with FLAGS ONLY (HARDWARE | ASYNCMFT):");
        let flags = windows::Win32::Media::MediaFoundation::MFT_ENUM_FLAG_HARDWARE.0 | 
                    windows::Win32::Media::MediaFoundation::MFT_ENUM_FLAG_ASYNCMFT.0 | 
                    0x00000001; // SORTANDFILTER

        let mut data = std::ptr::null_mut();
        let mut len = 0;
        let hr = MFTEnumEx(
            MFT_CATEGORY_VIDEO_ENCODER,
            windows::Win32::Media::MediaFoundation::MFT_ENUM_FLAG(flags),
            None, 
            None,
            &mut data,
            &mut len,
        );
        
        if hr.is_ok() {
             let activates = windows::core::Array::<windows::Win32::Media::MediaFoundation::IMFActivate>::from_raw_parts(data as _, len);
             println!("Output-only search found {} encoders.", activates.len());
             for (i, activate) in activates.as_slice().iter().enumerate() {
                if let Some(activate) = activate {
                    let mut name_ptr: PWSTR = PWSTR::null();
                    let mut name_len = 0;
                    activate.GetAllocatedString(
                        &windows::Win32::Media::MediaFoundation::MFT_FRIENDLY_NAME_Attribute,
                        &mut name_ptr,
                        &mut name_len,
                    ).ok();
                    let name = if !name_ptr.is_null() {
                        name_ptr.to_string().unwrap_or_default()
                    } else {
                        "Unknown".to_string()
                    };
                    println!("  {}: {}", i, name);
                }
             }
        }
    }
    Ok(())
}
