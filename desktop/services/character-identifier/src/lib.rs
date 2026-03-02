pub mod face_detector;
pub mod embedder;
pub mod matcher;

pub use face_detector::{FaceDetector, BBox};
pub use embedder::Embedder;
pub use matcher::{Matcher, IdentifiedCharacter};

use anyhow::Result;
use std::path::{Path, PathBuf};
use image::GenericImageView;
use serde::{Serialize, Deserialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceType {
    Cpu,
    Gpu,
}

pub struct CharacterIdentifier {
    face_detector: FaceDetector,
    embedder: Embedder,
    reference_embeddings: Vec<(String, Vec<f32>)>,
    models_dir: PathBuf,
}

impl CharacterIdentifier {
    pub fn new(models_dir: impl AsRef<Path>) -> Result<Self> {
        let models_dir = models_dir.as_ref().to_path_buf();
        let face_detector = FaceDetector::new(models_dir.join("yolov8_animeface.onnx"), false)?;
        let embedder = Embedder::new(models_dir.join("dinov2_vits14.onnx"), false)?;
        
        Ok(Self {
            face_detector,
            embedder,
            reference_embeddings: Vec::new(),
            models_dir,
        })
    }

    pub fn set_device(&mut self, device: DeviceType) -> Result<()> {
        let use_gpu = matches!(device, DeviceType::Gpu);
        self.face_detector = FaceDetector::new(self.models_dir.join("yolov8_animeface.onnx"), use_gpu)?;
        self.embedder = Embedder::new(self.models_dir.join("dinov2_vits14.onnx"), use_gpu)?;
        Ok(())
    }

    pub async fn register_references(
        &mut self,
        characters: &[(String, Vec<u8>)],
        cache_dir: &Path,
        _vndb_id: &str,
    ) -> Result<()> {
        let cache_file = cache_dir.join("embeddings.bin");
        
        if cache_file.exists() {
            if let Ok(data) = tokio::fs::read(&cache_file).await {
                if let Ok(embeddings) = bincode::deserialize::<Vec<(String, Vec<f32>)>>(&data[..]) {
                    self.reference_embeddings = embeddings;
                    return Ok(());
                }
            }
        }
        
        let mut new_embeddings = Vec::new();
        for (name, img_data) in characters {
            if let Ok(img) = image::load_from_memory(img_data) {
                if let Ok(mut boxes) = self.face_detector.detect(&img) {
                    boxes.sort_by(|a, b| b.area().partial_cmp(&a.area()).unwrap_or(std::cmp::Ordering::Equal));
                    if let Some(bbox) = boxes.first() {
                        let (w, h) = img.dimensions();
                        let img_w = w as f32;
                        let img_h = h as f32;
                        let crop_x = bbox.x1.max(0.0) as u32;
                        let crop_y = bbox.y1.max(0.0) as u32;
                        let crop_w = (bbox.x2 - bbox.x1).min(img_w - crop_x as f32) as u32;
                        let crop_h = (bbox.y2 - bbox.y1).min(img_h - crop_y as f32) as u32;
                        
                        let crop = img.crop_imm(crop_x, crop_y, crop_w, crop_h);
                        if let Ok(emb) = self.embedder.embed(&crop) {
                            new_embeddings.push((name.clone(), emb));
                        }
                    } else {
                        if let Ok(emb) = self.embedder.embed(&img) {
                            new_embeddings.push((name.clone(), emb));
                        }
                    }
                }
            }
        }
        
        self.reference_embeddings = new_embeddings;
        
        if let Ok(encoded) = bincode::serialize(&self.reference_embeddings) {
            let _ = tokio::fs::create_dir_all(cache_dir).await;
            let _ = tokio::fs::write(&cache_file, encoded).await;
        }
        
        Ok(())
    }
    
    pub fn identify(&mut self, screenshot: &[u8]) -> Result<Vec<IdentifiedCharacter>> {
        let img = image::load_from_memory(screenshot)?;
        let boxes = self.face_detector.detect(&img)?;
        
        let (w, h) = img.dimensions();
        let mut face_embeddings = Vec::new();
        
        for bbox in boxes {
            let crop_x = bbox.x1.max(0.0) as u32;
            let crop_y = bbox.y1.max(0.0) as u32;
            let crop_w = (bbox.x2 - bbox.x1).min(w as f32 - crop_x as f32) as u32;
            let crop_h = (bbox.y2 - bbox.y1).min(h as f32 - crop_y as f32) as u32;
            
            let crop = img.crop_imm(crop_x, crop_y, crop_w, crop_h);
            match self.embedder.embed(&crop) {
                Ok(emb) => {
                    face_embeddings.push((bbox, emb));
                }
                Err(e) => {
                    println!("Embedder error for face at {:?}: {:?}", bbox, e);
                }
            }
        }
        
        let results = Matcher::match_characters(&face_embeddings, &self.reference_embeddings, 0.6, w as f32, h as f32);
        Ok(results)
    }
}
