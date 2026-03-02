use crate::face_detector::BBox;

/// 識別結果
#[derive(Debug, Clone, PartialEq)]
pub struct IdentifiedCharacter {
    /// キャラクター名
    pub name: String,
    /// コサイン類似度 (0.0 - 1.0)
    pub confidence: f32,
    /// 左からの位置インデックス (0始まり、X座標の昇順でソート)
    pub position_index: usize,
    /// バウンディングボックス (x, y, w, h) 正規化座標
    pub bbox: (f32, f32, f32, f32),
}

pub struct Matcher;

impl Matcher {
    /// コサイン類似度を計算する (両ベクトルともL2正規化済みであることが前提)
    pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
    }

    /// 各顔に対してベストマッチを探す
    pub fn match_characters(
        faces: &[(BBox, Vec<f32>)],
        references: &[(String, Vec<f32>)],
        threshold: f32,
        img_w: f32, // 元画像の幅
        img_h: f32, // 元画像の高さ
    ) -> Vec<IdentifiedCharacter> {
        // 顔データをX座標(x1 + x2)/2 でソート
        let mut sorted_faces: Vec<_> = faces.iter().collect();
        sorted_faces.sort_by(|(a_box, _), (b_box, _)| {
            let cx_a = (a_box.x1 + a_box.x2) / 2.0;
            let cx_b = (b_box.x1 + b_box.x2) / 2.0;
            cx_a.partial_cmp(&cx_b).unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut results = Vec::new();

        for (position_index, (bbox, embedding)) in sorted_faces.into_iter().enumerate() {
            let mut best_match = "Unknown".to_string();
            let mut best_score = -1.0;

            for (ref_name, ref_emb) in references {
                let score = Self::cosine_similarity(embedding, ref_emb);
                if score > best_score {
                    best_score = score;
                    best_match = ref_name.clone();
                }
            }

            if best_score < threshold {
                best_match = "Unknown".to_string();
            }

            // bbox を正規化座標 (x, y, w, h) に変換
            let nx = bbox.x1 / img_w;
            let ny = bbox.y1 / img_h;
            let nw = (bbox.x2 - bbox.x1) / img_w;
            let nh = (bbox.y2 - bbox.y1) / img_h;

            results.push(IdentifiedCharacter {
                name: best_match,
                confidence: best_score.max(0.0), // 負にならないように
                position_index,
                bbox: (nx, ny, nw, nh),
            });
        }

        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        let c = vec![1.0, 0.0, 0.0];
        let d = vec![0.5f32.sqrt(), 0.5f32.sqrt(), 0.0];
        
        assert_eq!(Matcher::cosine_similarity(&a, &b), 0.0);
        assert_eq!(Matcher::cosine_similarity(&a, &c), 1.0);
        assert!((Matcher::cosine_similarity(&a, &d) - 0.5f32.sqrt()).abs() < 1e-5);
    }

    #[test]
    fn test_match_threshold() {
        let bbox = BBox { x1: 0.0, y1: 0.0, x2: 100.0, y2: 100.0, conf: 0.9 };
        let embedding = vec![1.0, 0.0];
        let faces = vec![(bbox, embedding)];
        
        // 類似度0.0
        let references = vec![("CharA".to_string(), vec![0.0, 1.0])];
        
        let results = Matcher::match_characters(&faces, &references, 0.6, 1000.0, 1000.0);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Unknown");
    }

    #[test]
    fn test_position_index_ordering() {
        let bbox1 = BBox { x1: 50.0, y1: 0.0, x2: 100.0, y2: 100.0, conf: 0.9 }; // cx = 75
        let bbox2 = BBox { x1: 0.0, y1: 0.0, x2: 40.0, y2: 100.0, conf: 0.9 };  // cx = 20
        
        let faces = vec![
            (bbox1, vec![1.0, 0.0]),
            (bbox2, vec![0.0, 1.0]),
        ];
        
        let references = vec![];
        let results = Matcher::match_characters(&faces, &references, 0.6, 1000.0, 1000.0);
        
        assert_eq!(results.len(), 2);
        // bbox2 (cx=20) should be first
        assert_eq!(results[0].position_index, 0);
        // bbox1 (cx=75) should be second
        assert_eq!(results[1].position_index, 1);
        
        // Ensure nx matches bbox2
        assert_eq!(results[0].bbox.0, 0.0);
        assert_eq!(results[1].bbox.0, 0.05); // 50.0 / 1000.0 = 0.05
    }

    #[test]
    fn test_best_match_selection() {
        let bbox = BBox { x1: 0.0, y1: 0.0, x2: 100.0, y2: 100.0, conf: 0.9 };
        let embedding = vec![0.8, 0.6]; // norm=1.0
        let faces = vec![(bbox, embedding)];
        
        let references = vec![
            ("CharA".to_string(), vec![0.0, 1.0]), // dot = 0.6
            ("CharB".to_string(), vec![0.8, 0.6]), // dot = 1.0
            ("CharC".to_string(), vec![1.0, 0.0]), // dot = 0.8
        ];
        
        let results = Matcher::match_characters(&faces, &references, 0.5, 1000.0, 1000.0);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "CharB");
        assert!((results[0].confidence - 1.0).abs() < 1e-5);
    }
}
