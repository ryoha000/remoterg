use character_identifier::{CharacterIdentifier, DeviceType, FaceDetector};
use std::path::PathBuf;
use std::fs;
use std::time::Instant;
use image::GenericImageView;
use std::sync::Once;


static INIT_TRACING: Once = Once::new();

fn init_tracing() {
    INIT_TRACING.call_once(|| {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_test_writer()
            .init();
    });
}

#[tokio::test]
async fn test_character_identification_pipeline() {
    init_tracing();
    let total_start = Instant::now();
    let init_start = Instant::now();

    // 1. モデルファイルのパスを指定
    let models_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../assets/character_identifier_models")
        .canonicalize()
        .expect("モデルディレクトリのパス解決に失敗しました");

    let mut identifier = CharacterIdentifier::new(&models_dir)
        .expect("モデルの読み込みに失敗しました。パスやファイル名を確認してください");
        
    identifier.set_device(DeviceType::Gpu).expect("GPUの初期化に失敗しました");

    let test_data_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("data");
    let characters_dir = test_data_dir.join("characters");
    let cache_dir = test_data_dir.join("cache");
    
    // クロップ画像保存用ディレクトリ
    let char_cropped_dir = characters_dir.join("cropped");
    let screenshot_cropped_dir = test_data_dir.join("cropped");
    
    let _ = fs::remove_dir_all(&char_cropped_dir);
    let _ = fs::remove_dir_all(&screenshot_cropped_dir);
    fs::create_dir_all(&char_cropped_dir).expect("ディレクトリ作成失敗");
    fs::create_dir_all(&screenshot_cropped_dir).expect("ディレクトリ作成失敗");

    // 前回のキャッシュが残っていると画像を変えても古い特徴量が使われるため、テストの最初はキャッシュを消す
    let _ = fs::remove_dir_all(&cache_dir);

    // デバッグ用に直接 FaceDetector を呼んで画像を保存する
    let mut face_detector = FaceDetector::new(models_dir.join("yolov8_animeface.onnx"), true)
        .expect("FaceDetectorの初期化に失敗しました");

    println!("初期化完了: {:?}", init_start.elapsed());

    let load_start = Instant::now();

    // 2. charactersディレクトリからすべての画像をキャラクターとして読み込む
    let mut characters = Vec::new();
    if characters_dir.exists() {
        for entry in fs::read_dir(&characters_dir).expect("charactersディレクトリの読み込みに失敗しました") {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    let ext = ext.to_lowercase();
                    if ext == "png" || ext == "jpg" || ext == "jpeg" || ext == "webp" {
                        let name = path.file_stem().unwrap().to_string_lossy().to_string();
                        let img_data = fs::read(&path).expect("画像ファイルの読み込みに失敗しました");
                        characters.push((name.clone(), img_data.clone()));
                        
                        // ===== デバッグ用: リファレンス画像の顔検出結果を保存 =====
                        if let Ok(img) = image::load_from_memory(&img_data) {
                            if let Ok(mut boxes) = face_detector.detect(&img) {
                                boxes.sort_by(|a, b| b.area().partial_cmp(&a.area()).unwrap_or(std::cmp::Ordering::Equal));
                                if let Some(bbox) = boxes.first() {
                                    let (w, h) = img.dimensions();
                                    let crop_x = bbox.x1.max(0.0) as u32;
                                    let crop_y = bbox.y1.max(0.0) as u32;
                                    let crop_w = (bbox.x2 - bbox.x1).min(w as f32 - crop_x as f32) as u32;
                                    let crop_h = (bbox.y2 - bbox.y1).min(h as f32 - crop_y as f32) as u32;
                                    
                                    if crop_w > 0 && crop_h > 0 {
                                        let crop = img.crop_imm(crop_x, crop_y, crop_w, crop_h);
                                        let save_path = char_cropped_dir.join(format!("{}_face.png", name));
                                        let _ = crop.save(&save_path);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    } else {
        fs::create_dir_all(&characters_dir).expect("charactersディレクトリの作成に失敗しました");
        println!("警告: {:?} が存在しなかったため作成しました。画像を配置してください。", characters_dir);
    }

    assert!(!characters.is_empty(), "テスト用のキャラクター画像が1枚も見つかりません。{:?} に画像を配置してください", characters_dir);
    println!("キャラクター画像読み込み完了 ({}人): {:?}", characters.len(), load_start.elapsed());

    let register_start = Instant::now();

    // 特徴量の抽出とキャッシュの保存を実行
    identifier.register_references(&characters, &cache_dir, "test_game_id")
        .await
        .expect("リファレンス画像の登録に失敗しました");

    println!("リファレンス画像登録/特徴量抽出完了: {:?}", register_start.elapsed());

    // 3. スクリーンショットの読み込みと推論
    let screenshot_path = test_data_dir.join("screenshot.jpeg");
    assert!(screenshot_path.exists(), "テスト用スクショ {:?} が見つかりません", screenshot_path);
    
    let ss_load_start = Instant::now();
    let screenshot = fs::read(&screenshot_path).unwrap();
    println!("スクリーンショット読み込み完了: {:?}", ss_load_start.elapsed());

    // ===== デバッグ用: スクリーンショットの顔検出結果を保存 =====
    if let Ok(img) = image::load_from_memory(&screenshot) {
        if let Ok(boxes) = face_detector.detect(&img) {
            let (w, h) = img.dimensions();
            println!("スクショから {} 個の顔枠(BBox)を検出しました", boxes.len());
            for (i, bbox) in boxes.iter().enumerate() {
                let crop_x = bbox.x1.max(0.0) as u32;
                let crop_y = bbox.y1.max(0.0) as u32;
                let crop_w = (bbox.x2 - bbox.x1).min(w as f32 - crop_x as f32) as u32;
                let crop_h = (bbox.y2 - bbox.y1).min(h as f32 - crop_y as f32) as u32;
                
                if crop_w > 0 && crop_h > 0 {
                    let crop = img.crop_imm(crop_x, crop_y, crop_w, crop_h);
                    let save_path = screenshot_cropped_dir.join(format!("face_{}.png", i));
                    let _ = crop.save(&save_path);
                }
            }
        } else {
            println!("スクショからの顔検出に失敗しました（FaceDetectorエラー）");
        }
    }

    let inference_start = Instant::now();
    let results = identifier.identify(&screenshot).expect("推論処理に失敗しました");
    println!("スクリーンショット推論処理完了: {:?}", inference_start.elapsed());

    // デバッグ用に結果を出力（cargo test -- --nocapture で確認できます）
    println!("識別結果: {:#?}", results);
    println!("全体の処理時間: {:?}", total_start.elapsed());

    // ==========================================
    // 4. 期待値の検証 (アサーション) 
    // ※ 用意した画像・スクショに合わせて以下の期待値を書き換えてください
    // ==========================================

    // 例1: 検出された顔（キャラクター）の数が想定通りか
    assert_eq!(results.len(), 3, "スクショからは3人のキャラクターが検出されるべきです");

    // 例2: 期待するキャラクターが判定結果に含まれているか
    let target_chars = vec!["クラリス・ツァインブルグ", "ヴァース"];
    for target_char in target_chars {
        let has_target = results.iter().any(|c| c.name == target_char);
        assert!(has_target, "{} が見つかりませんでした", target_char);
    }

    // 例3: 特定のキャラクターの類似度(confidence)が十分高いか
    // if let Some(target_result) = results.iter().find(|c| c.name == target_char) {
    //     assert!(target_result.confidence > 0.6, "{} の類似度が低すぎます ({})", target_char, target_result.confidence);
    // }

    // 例4: 画像の左から何番目にいるか (position_index) を検証する
    // ※画面の左側のキャラが 0、右側が 1 になります
    // if results.len() >= 2 {
    //     assert_eq!(results[0].name, "左にいるべきキャラ名");
    //     assert_eq!(results[1].name, "右にいるべきキャラ名");
    // }
}
