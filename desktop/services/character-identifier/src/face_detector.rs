use anyhow::{Context, Result};
use image::{imageops::FilterType, DynamicImage, GenericImageView};
use ndarray::{Array, Array4};
use ort::{
    execution_providers::{CPUExecutionProvider, CUDAExecutionProvider},
    session::builder::GraphOptimizationLevel,
    session::Session,
    value::Value,
};
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct BBox {
    pub x1: f32, // x-min
    pub y1: f32, // y-min
    pub x2: f32, // x-max
    pub y2: f32, // y-max
    pub conf: f32,
}

impl BBox {
    pub fn area(&self) -> f32 {
        (self.x2 - self.x1).max(0.0) * (self.y2 - self.y1).max(0.0)
    }
}

pub struct FaceDetector {
    session: Session,
    input_size: u32,
    conf_threshold: f32,
    iou_threshold: f32,
}

impl FaceDetector {
    /// モデルを読み込んで初期化する
    pub fn new(model_path: impl AsRef<Path>, use_gpu: bool) -> Result<Self> {
        let mut builder = Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level3)?
            .with_intra_threads(4)?;

        if use_gpu {
            builder =
                builder.with_execution_providers([CUDAExecutionProvider::default().build()])?;
        } else {
            builder =
                builder.with_execution_providers([CPUExecutionProvider::default().build()])?;
        }

        let session = builder.commit_from_file(model_path)?;

        Ok(Self {
            session,
            input_size: 640,
            conf_threshold: 0.5,
            iou_threshold: 0.45,
        })
    }

    /// 前処理：指定サイズにリサイズ（アスペクト比維持でパディング）、[0,1]への正規化、CHW変換
    pub fn preprocess(image: &DynamicImage, target_size: u32) -> (Array4<f32>, f32, f32, f32) {
        let (w, h) = image.dimensions();
        let scale = (target_size as f32 / w as f32).min(target_size as f32 / h as f32);

        let new_w = (w as f32 * scale).round() as u32;
        let new_h = (h as f32 * scale).round() as u32;

        // Resize
        let resized = image.resize_exact(new_w, new_h, FilterType::Triangle);
        let resized_rgb = resized.to_rgb8();

        // Calculate padding
        let pad_w = target_size - new_w;
        let pad_h = target_size - new_h;
        let pad_left = pad_w / 2;
        let pad_top = pad_h / 2;

        let mut tensor = Array::zeros((1, 3, target_size as usize, target_size as usize));

        for (x, y, pixel) in resized_rgb.enumerate_pixels() {
            let px = x + pad_left;
            let py = y + pad_top;

            // Normalize to [0, 1] and CHW format
            tensor[[0, 0, py as usize, px as usize]] = pixel[0] as f32 / 255.0;
            tensor[[0, 1, py as usize, px as usize]] = pixel[1] as f32 / 255.0;
            tensor[[0, 2, py as usize, px as usize]] = pixel[2] as f32 / 255.0;
        }

        (tensor, scale, pad_left as f32, pad_top as f32)
    }

    /// IoU (Intersection over Union)
    pub fn iou(a: &BBox, b: &BBox) -> f32 {
        let x1 = a.x1.max(b.x1);
        let y1 = a.y1.max(b.y1);
        let x2 = a.x2.min(b.x2);
        let y2 = a.y2.min(b.y2);

        let inter_area = (x2 - x1).max(0.0) * (y2 - y1).max(0.0);
        if inter_area <= 0.0 {
            return 0.0;
        }

        let union_area = a.area() + b.area() - inter_area;
        inter_area / union_area
    }

    /// NMS (Non-Maximum Suppression)
    pub fn nms(mut boxes: Vec<BBox>, iou_threshold: f32) -> Vec<BBox> {
        boxes.sort_by(|a, b| {
            b.conf
                .partial_cmp(&a.conf)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut keep = Vec::new();

        while !boxes.is_empty() {
            let current = boxes.remove(0);
            let current_clone = current.clone();
            keep.push(current);

            boxes.retain(|b| Self::iou(&current_clone, b) < iou_threshold);
        }

        keep
    }

    /// YOLOv8 の出力テンソルからバウンディングボックスを抽出
    /// output_tensor: [1, 5, 8400] などの形状を想定 (1: batch, 5: cx,cy,w,h,conf, 8400: anchors)
    pub fn postprocess(
        output_tensor: ndarray::ArrayView3<f32>,
        scale: f32,
        pad_left: f32,
        pad_top: f32,
        img_w: f32,
        img_h: f32,
        conf_threshold: f32,
        iou_threshold: f32,
    ) -> Vec<BBox> {
        let mut boxes = Vec::new();
        let num_anchors = output_tensor.shape()[2];

        // YOLOv8 output: [batch_size=1, num_classes+4, num_anchors]
        for i in 0..num_anchors {
            let conf = output_tensor[[0, 4, i]]; // Assuming 1 class + 4 bbox coords
            if conf >= conf_threshold {
                let cx = output_tensor[[0, 0, i]];
                let cy = output_tensor[[0, 1, i]];
                let w = output_tensor[[0, 2, i]];
                let h = output_tensor[[0, 3, i]];

                // Adjust for padding and scale
                let cx = (cx - pad_left) / scale;
                let cy = (cy - pad_top) / scale;
                let w = w / scale;
                let h = h / scale;

                let x1 = (cx - w / 2.0).max(0.0).min(img_w);
                let y1 = (cy - h / 2.0).max(0.0).min(img_h);
                let x2 = (cx + w / 2.0).max(0.0).min(img_w);
                let y2 = (cy + h / 2.0).max(0.0).min(img_h);

                boxes.push(BBox {
                    x1,
                    y1,
                    x2,
                    y2,
                    conf,
                });
            }
        }

        Self::nms(boxes, iou_threshold)
    }

    /// 画像から顔を検出する
    pub fn detect(&mut self, image: &DynamicImage) -> Result<Vec<BBox>> {
        let (w, h) = image.dimensions();
        let (tensor, scale, pad_left, pad_top) = Self::preprocess(image, self.input_size);

        let shape: Vec<i64> = tensor.shape().iter().map(|&x| x as i64).collect();
        let data = tensor.into_raw_vec();

        let input_tensor = Value::from_array((shape, data))?;
        let outputs = self.session.run(ort::inputs![input_tensor])?;

        // 初めの出力を取得
        let output = outputs.into_iter().next().context("No output tensor")?.1;
        let (shape, data) = output.try_extract_tensor::<f32>()?;

        // ArrayView に変換
        let nd_shape = shape.iter().map(|&x| x as usize).collect::<Vec<_>>();
        let view = ndarray::ArrayView::from_shape(nd_shape, data)?;
        let view3 = view.into_dimensionality::<ndarray::Ix3>()?;

        let boxes = Self::postprocess(
            view3,
            scale,
            pad_left,
            pad_top,
            w as f32,
            h as f32,
            self.conf_threshold,
            self.iou_threshold,
        );

        Ok(boxes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbImage;

    #[test]
    fn test_preprocess_image() {
        let img = RgbImage::new(800, 600);
        let dyn_img = DynamicImage::ImageRgb8(img);

        let (tensor, scale, pad_left, pad_top) = FaceDetector::preprocess(&dyn_img, 640);

        assert_eq!(tensor.shape(), &[1, 3, 640, 640]);
        // scale = min(640/800, 640/600) = min(0.8, 1.066) = 0.8
        assert!((scale - 0.8).abs() < 1e-5);

        // new_w = 800 * 0.8 = 640
        // new_h = 600 * 0.8 = 480
        // pad_left = (640 - 640)/2 = 0
        // pad_top = (640 - 480)/2 = 80
        assert_eq!(pad_left, 0.0);
        assert_eq!(pad_top, 80.0);
    }

    #[test]
    fn test_nms() {
        let boxes = vec![
            BBox {
                x1: 0.0,
                y1: 0.0,
                x2: 100.0,
                y2: 100.0,
                conf: 0.9,
            },
            BBox {
                x1: 5.0,
                y1: 5.0,
                x2: 95.0,
                y2: 95.0,
                conf: 0.8,
            }, // 高IoU, 削除されるべき
            BBox {
                x1: 200.0,
                y1: 200.0,
                x2: 300.0,
                y2: 300.0,
                conf: 0.7,
            }, // 低IoU, 残るべき
        ];

        let result = FaceDetector::nms(boxes, 0.5);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].conf, 0.9);
        assert_eq!(result[1].conf, 0.7);
    }

    #[test]
    fn test_postprocess_output() {
        // [1, 5, 2] のダミー出力テンソル (バッチ1, クラス数1+bbox4=5, アンカー数2)
        // 1つ目のアンカー: cx=50, cy=50, w=20, h=30, conf=0.8
        // 2つ目のアンカー: cx=150, cy=100, w=10, h=10, conf=0.3 (閾値未満)
        let mut data = Array::zeros((1, 5, 2));
        data[[0, 0, 0]] = 50.0;
        data[[0, 1, 0]] = 50.0;
        data[[0, 2, 0]] = 20.0;
        data[[0, 3, 0]] = 30.0;
        data[[0, 4, 0]] = 0.8;

        data[[0, 0, 1]] = 150.0;
        data[[0, 1, 1]] = 100.0;
        data[[0, 2, 1]] = 10.0;
        data[[0, 3, 1]] = 10.0;
        data[[0, 4, 1]] = 0.3;

        let boxes = FaceDetector::postprocess(data.view(), 1.0, 0.0, 0.0, 640.0, 640.0, 0.5, 0.45);

        assert_eq!(boxes.len(), 1);
        assert_eq!(boxes[0].conf, 0.8);
        assert_eq!(boxes[0].x1, 40.0);
        assert_eq!(boxes[0].y1, 35.0);
        assert_eq!(boxes[0].x2, 60.0);
        assert_eq!(boxes[0].y2, 65.0);
    }
}
