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

    fn check_hard_filter(ref_name: &str, ref_tags: &[String], face_tags: &[String]) -> bool {
        let groups = vec![
            vec![
                "blonde_hair",
                "brown_hair",
                "black_hair",
                "blue_hair",
                "pink_hair",
                "purple_hair",
                "white_hair",
                "grey_hair",
                "red_hair",
                "silver_hair",
                "green_hair",
                "orange_hair",
                "aqua_hair",
            ],
            vec![
                "blue_eyes",
                "red_eyes",
                "brown_eyes",
                "green_eyes",
                "purple_eyes",
                "yellow_eyes",
                "pink_eyes",
                "aqua_eyes",
                "black_eyes",
                "orange_eyes",
                "grey_eyes",
            ],
            vec!["long_hair", "short_hair", "medium_hair", "very_long_hair"],
        ];

        for group in groups {
            let ref_has_group = ref_tags.iter().any(|t| group.contains(&t.as_str()));
            let face_has_group = face_tags.iter().any(|t| group.contains(&t.as_str()));

            if ref_has_group && face_has_group {
                // 両方がこのグループの属性を持っている場合、共通するものがあるかチェック
                let has_intersection = ref_tags
                    .iter()
                    .any(|t| group.contains(&t.as_str()) && face_tags.contains(t));
                if !has_intersection {
                    tracing::debug!(
                        "Hard Filter REJECTED '{}': group conflict in {:?}",
                        ref_name,
                        group
                    );
                    return false; // 致命的な矛盾
                }
            }
        }
        true
    }

    /// 各顔に対してベストマッチを探す
    pub fn match_characters(
        faces: &[(BBox, Vec<f32>, Vec<String>)],
        references: &[(String, Vec<f32>, Vec<String>)],
        threshold: f32,
        img_w: f32,
        img_h: f32,
    ) -> Vec<IdentifiedCharacter> {
        let mut sorted_faces: Vec<_> = faces.iter().collect();
        sorted_faces.sort_by(|(a_box, _, _), (b_box, _, _)| {
            let cx_a = (a_box.x1 + a_box.x2) / 2.0;
            let cx_b = (b_box.x1 + b_box.x2) / 2.0;
            cx_a.partial_cmp(&cx_b).unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut results = Vec::new();

        for (position_index, (bbox, embedding, face_tags)) in sorted_faces.into_iter().enumerate() {
            let mut best_match = "Unknown".to_string();
            let mut best_score = -1.0;

            tracing::debug!("--- Face {} Tags: {:?} ---", position_index, face_tags);

            for (ref_name, ref_emb, ref_tags) in references {
                if !Self::check_hard_filter(ref_name, ref_tags, face_tags) {
                    continue; // 矛盾がある場合はスキップ
                }

                let base_score = Self::cosine_similarity(embedding, ref_emb);

                // Soft Weighting: 一致率 (Jaccard係数的なもの) を係数として掛け合わせる
                // 参照特徴のタグ集合と推論されたタグ集合の積を計算
                let intersection_count = ref_tags.iter().filter(|t| face_tags.contains(t)).count();
                let union_count = ref_tags.len() + face_tags.len() - intersection_count;

                let match_rate = if union_count > 0 {
                    intersection_count as f32 / union_count as f32
                } else {
                    1.0 // タグが一切ない場合はペナルティなし
                };

                // 完全一致で 1.0, 不一致で下がるような係数 (ここでは 0.6 + 0.4 * match_rate とし、急激に0にならないように調整)
                let weight = 0.6 + 0.4 * match_rate;
                let score = base_score * weight;

                tracing::debug!(
                    "Match candidate '{}': base_score={:.4}, match_rate={:.4} (intersect={}, union={}), weight={:.4} => final_score={:.4}",
                    ref_name, base_score, match_rate, intersection_count, union_count, weight, score
                );

                if score > best_score {
                    best_score = score;
                    best_match = ref_name.clone();
                }
            }

            if best_score < threshold {
                best_match = "Unknown".to_string();
            }

            let nx = bbox.x1 / img_w;
            let ny = bbox.y1 / img_h;
            let nw = (bbox.x2 - bbox.x1) / img_w;
            let nh = (bbox.y2 - bbox.y1) / img_h;

            results.push(IdentifiedCharacter {
                name: best_match,
                confidence: best_score.max(0.0),
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
        let bbox = BBox {
            x1: 0.0,
            y1: 0.0,
            x2: 100.0,
            y2: 100.0,
            conf: 0.9,
        };
        let embedding = vec![1.0, 0.0];
        let faces: Vec<(BBox, Vec<f32>, Vec<String>)> = vec![(bbox, embedding, Vec::new())];

        // 類似度0.0
        let references: Vec<(String, Vec<f32>, Vec<String>)> =
            vec![("CharA".to_string(), vec![0.0, 1.0], Vec::new())];

        let results = Matcher::match_characters(&faces, &references, 0.6, 1000.0, 1000.0);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Unknown");
    }

    #[test]
    fn test_position_index_ordering() {
        let bbox1 = BBox {
            x1: 50.0,
            y1: 0.0,
            x2: 100.0,
            y2: 100.0,
            conf: 0.9,
        }; // cx = 75
        let bbox2 = BBox {
            x1: 0.0,
            y1: 0.0,
            x2: 40.0,
            y2: 100.0,
            conf: 0.9,
        }; // cx = 20

        let faces: Vec<(BBox, Vec<f32>, Vec<String>)> = vec![
            (bbox1, vec![1.0, 0.0], Vec::new()),
            (bbox2, vec![0.0, 1.0], Vec::new()),
        ];

        let references: Vec<(String, Vec<f32>, Vec<String>)> = vec![];
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
        let bbox = BBox {
            x1: 0.0,
            y1: 0.0,
            x2: 100.0,
            y2: 100.0,
            conf: 0.9,
        };
        let embedding = vec![0.8, 0.6]; // norm=1.0
        let faces: Vec<(BBox, Vec<f32>, Vec<String>)> = vec![(bbox, embedding, Vec::new())];

        let references: Vec<(String, Vec<f32>, Vec<String>)> = vec![
            ("CharA".to_string(), vec![0.0, 1.0], Vec::new()), // dot = 0.6
            ("CharB".to_string(), vec![0.8, 0.6], Vec::new()), // dot = 1.0
            ("CharC".to_string(), vec![1.0, 0.0], Vec::new()), // dot = 0.8
        ];

        let results = Matcher::match_characters(&faces, &references, 0.5, 1000.0, 1000.0);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "CharB");
        assert!((results[0].confidence - 1.0).abs() < 1e-5);
    }

    #[test]
    fn test_hard_filter_and_soft_weight() {
        let bbox = BBox {
            x1: 0.0,
            y1: 0.0,
            x2: 100.0,
            y2: 100.0,
            conf: 0.9,
        };
        let embedding = vec![1.0, 0.0];
        // 顔のタグ: blue_hair
        let faces = vec![(
            bbox.clone(),
            embedding.clone(),
            vec!["blue_hair".to_string(), "1girl".to_string()],
        )];

        let references = vec![
            // CharA: red_hair (contradicts blue_hair, should be hard filtered), high base score
            (
                "CharA".to_string(),
                vec![1.0, 0.0],
                vec!["red_hair".to_string(), "1girl".to_string()],
            ),
            // CharB: blue_hair (matches completely), medium base score (0.8)
            (
                "CharB".to_string(),
                vec![0.8, 0.6],
                vec!["blue_hair".to_string(), "1girl".to_string()],
            ),
            // CharC: no hair tag, match is okay, no boost, low base score (0.6)
            (
                "CharC".to_string(),
                vec![0.6, 0.8],
                vec!["1girl".to_string()],
            ),
        ];

        let results = Matcher::match_characters(&faces, &references, 0.1, 1000.0, 1000.0);
        assert_eq!(results.len(), 1);

        // CharA is filtered out.
        // CharB tag JACCARD = 2 / 2 = 1.0
        // weight = 0.6 + 0.4 * 1.0 = 1.0
        // score = 0.8 * 1.0 = 0.8
        assert_eq!(results[0].name, "CharB");
        assert!((results[0].confidence - 0.8).abs() < 1e-5);
    }
}
