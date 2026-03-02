use anyhow::{Context, Result};
use image::{DynamicImage, imageops::FilterType};
use ndarray::{Array, Array4};
use ort::{session::Session, session::builder::GraphOptimizationLevel, value::Value, execution_providers::{CPUExecutionProvider, CUDAExecutionProvider}};
use std::path::Path;

pub struct Embedder {
    session: Session,
    input_size: u32,
}

impl Embedder {
    pub fn new(model_path: impl AsRef<Path>, use_gpu: bool) -> Result<Self> {
        let mut builder = Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level3)?
            .with_intra_threads(4)?;

        if use_gpu {
            builder = builder.with_execution_providers([CUDAExecutionProvider::default().build()])?;
        } else {
            builder = builder.with_execution_providers([CPUExecutionProvider::default().build()])?;
        }

        let session = builder.commit_from_file(model_path)?;

        Ok(Self {
            session,
            input_size: 224, // DINOv2 default
        })
    }

    /// 前処理: 指定サイズにリサイズ後、ImageNet 正規化
    pub fn preprocess(image: &DynamicImage, target_size: u32) -> Array4<f32> {
        let resized = image.resize_exact(target_size, target_size, FilterType::Triangle);
        let resized_rgb = resized.to_rgb8();

        let mut tensor = Array::zeros((1, 3, target_size as usize, target_size as usize));

        let mean = [0.485, 0.456, 0.406];
        let std = [0.229, 0.224, 0.225];

        for (x, y, pixel) in resized_rgb.enumerate_pixels() {
            for c in 0..3 {
                let val = pixel[c] as f32 / 255.0;
                tensor[[0, c, y as usize, x as usize]] = (val - mean[c]) / std[c];
            }
        }

        tensor
    }

    /// L2正規化
    pub fn l2_normalize(vec: &mut [f32]) {
        let sum_sq: f32 = vec.iter().map(|&x| x * x).sum::<f32>();
        let norm = sum_sq.sqrt().max(1e-12);
        for x in vec.iter_mut() {
            *x /= norm;
        }
    }

    /// 顔画像のcrop（または全体）からEmbedding（例: 384次元ベクトル）を生成する
    pub fn embed(&mut self, image: &DynamicImage) -> Result<Vec<f32>> {
        let tensor = Self::preprocess(image, self.input_size);
        
        let shape: Vec<i64> = tensor.shape().iter().map(|&x| x as i64).collect();
        let data = tensor.into_raw_vec();
        
        let input_tensor = Value::from_array((shape, data))?;
        
        let mask_shape: Vec<i64> = vec![1];
        let mask_data: Vec<bool> = vec![false];
        let mask_tensor = Value::from_array((mask_shape, mask_data))?;

        let outputs = self.session.run(ort::inputs![
            "input" => input_tensor,
            "masks" => mask_tensor
        ])?;
        
        let output = outputs.into_iter().next().context("No output tensor")?.1;
        let (_shape, data) = output.try_extract_tensor::<f32>()?;
        
        let mut vec = data.to_vec();
        Self::l2_normalize(&mut vec);
        
        Ok(vec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbImage;

    #[test]
    fn test_preprocess_normalize() {
        let mut img = RgbImage::new(224, 224);
        for p in img.pixels_mut() {
            *p = image::Rgb([0, 0, 0]);
        }
        let dyn_img = DynamicImage::ImageRgb8(img);
        
        let tensor = Embedder::preprocess(&dyn_img, 224);
        let mean = [0.485, 0.456, 0.406];
        let std = [0.229, 0.224, 0.225];
        
        assert_eq!(tensor.shape(), &[1, 3, 224, 224]);
        
        let val_r = tensor[[0, 0, 100, 100]];
        let val_g = tensor[[0, 1, 100, 100]];
        let val_b = tensor[[0, 2, 100, 100]];
        
        assert!((val_r - (0.0 - mean[0]) / std[0]).abs() < 1e-5);
        assert!((val_g - (0.0 - mean[1]) / std[1]).abs() < 1e-5);
        assert!((val_b - (0.0 - mean[2]) / std[2]).abs() < 1e-5);
    }

    #[test]
    fn test_l2_normalize() {
        let mut vec = vec![3.0, 4.0];
        Embedder::l2_normalize(&mut vec);
        
        assert!((vec[0] - 0.6).abs() < 1e-5);
        assert!((vec[1] - 0.8).abs() < 1e-5);
        
        let sum_sq: f32 = vec.iter().map(|&x| x * x).sum();
        assert!((sum_sq - 1.0).abs() < 1e-5);
    }
}
