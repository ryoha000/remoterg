use anyhow::{Context, Result};
use image::{DynamicImage, RgbImage, imageops::FilterType, GenericImageView};
use ndarray::{Array, Array4};
use ort::{session::Session, session::builder::GraphOptimizationLevel, value::Value, execution_providers::{CPUExecutionProvider, CUDAExecutionProvider}};
use std::path::Path;
use csv::ReaderBuilder;

#[derive(Debug, Clone, PartialEq)]
pub struct TagResult {
    pub tag: String,
    pub score: f32,
}

pub struct Tagger {
    session: Session,
    input_size: u32,
    tags: Vec<String>,
}

impl Tagger {
    pub fn new(model_path: impl AsRef<Path>, tags_path: impl AsRef<Path>, use_gpu: bool) -> Result<Self> {
        let mut builder = Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level3)?
            .with_intra_threads(4)?;

        if use_gpu {
            builder = builder.with_execution_providers([CUDAExecutionProvider::default().build()])?;
        } else {
            builder = builder.with_execution_providers([CPUExecutionProvider::default().build()])?;
        }

        let session = builder.commit_from_file(model_path)?;
        
        let mut rdr = ReaderBuilder::new()
            .has_headers(true)
            .from_path(tags_path)?;
        
        let mut tags = Vec::new();
        for result in rdr.records() {
            let record = result?;
            if let Some(tag) = record.get(1) { // 2番目のカラムがname
                tags.push(tag.to_string());
            }
        }

        Ok(Self {
            session,
            input_size: 448, // WD Tagger v3 default
            tags,
        })
    }

    /// 前処理: アスペクト比を維持して最大サイズにリサイズ後、白(255)でパディングし、BGR(NHWC)のテンソルにする
    pub fn preprocess(image: &DynamicImage, target_size: u32) -> Array4<f32> {
        let (w, h) = image.dimensions();
        let scale = (target_size as f32 / w as f32).min(target_size as f32 / h as f32);
        
        let new_w = (w as f32 * scale).round() as u32;
        let new_h = (h as f32 * scale).round() as u32;
        
        let resized = image.resize_exact(new_w, new_h, FilterType::Triangle);
        let resized_rgb = resized.to_rgb8();
        
        // 白でパディング
        let mut padded = RgbImage::from_pixel(target_size, target_size, image::Rgb([255, 255, 255]));
        
        // 中央に配置
        let offset_x = (target_size - new_w) / 2;
        let offset_y = (target_size - new_h) / 2;
        
        image::imageops::overlay(&mut padded, &resized_rgb, offset_x as i64, offset_y as i64);

        // NHWC, BGR
        let mut tensor = Array::zeros((1, target_size as usize, target_size as usize, 3));

        for (x, y, pixel) in padded.enumerate_pixels() {
            let r = pixel[0] as f32;
            let g = pixel[1] as f32;
            let b = pixel[2] as f32;
            
            // BGR
            tensor[[0, y as usize, x as usize, 0]] = b;
            tensor[[0, y as usize, x as usize, 1]] = g;
            tensor[[0, y as usize, x as usize, 2]] = r;
        }

        tensor
    }

    pub fn predict(&mut self, image: &DynamicImage, threshold: f32) -> Result<Vec<TagResult>> {
        let tensor = Self::preprocess(image, self.input_size);
        
        let shape: Vec<i64> = tensor.shape().iter().map(|&x| x as i64).collect();
        let data = tensor.into_raw_vec();
        
        let input_tensor = Value::from_array((shape, data))?;

        let outputs = self.session.run(ort::inputs![
            "input" => input_tensor,
        ])?;
        
        let output = outputs.into_iter().next().context("No output tensor")?.1;
        let (_shape, data) = output.try_extract_tensor::<f32>()?;
        
        let mut results = Vec::new();
        // モデルの出力とCSVのタグ数が一致するか確認（10861とか）
        let len = self.tags.len().min(data.len());
        
        for i in 0..len {
            let score = data[i];
            if score >= threshold {
                results.push(TagResult {
                    tag: self.tags[i].clone(),
                    score,
                });
            }
        }
        
        // スコアの降順でソート
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());

        Ok(results)
    }
}
