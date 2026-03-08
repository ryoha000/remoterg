use crate::matcher::{MatchMethod, MatchResult};
use crate::normalize::normalize_for_match;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ScoredCandidate {
    pub vndb_id: String,
    pub final_score: f64,
    pub best_match: MatchResult,
    pub all_matches: Vec<MatchResult>,
}

pub fn score_and_select_all(matches: Vec<MatchResult>) -> Option<Vec<ScoredCandidate>> {
    if matches.is_empty() {
        return None;
    }

    // vndb_id でグルーピング
    let mut grouped: HashMap<String, Vec<MatchResult>> = HashMap::new();

    for mut m in matches {
        let base_type = m.match_type.split(':').next().unwrap_or(&m.match_type);

        let type_weight = match base_type {
            "title" => 1.0,
            "title_latin" => 0.95,
            "alias" => 0.9,
            "brand" => 0.6,
            "brand_latin" => 0.55,
            "brand_alias" => 0.5,
            _ => 0.5,
        };

        let method_base = match m.match_method {
            MatchMethod::Exact => 1.0,
            MatchMethod::ExactSpaceAgnostic => 0.95,
            MatchMethod::Prefix => 0.85,
            MatchMethod::TokenInName | MatchMethod::NameInToken => 0.7,
            MatchMethod::TokenInNameSpaceAgnostic | MatchMethod::NameInTokenSpaceAgnostic => 0.65,
        };

        let mut w_score = m.score * type_weight * method_base;

        // Proximity penalty
        if m.match_method != MatchMethod::Exact
            && m.match_method != MatchMethod::ExactSpaceAgnostic
            && m.match_method != MatchMethod::Prefix
        {
            let norm_token_chars = normalize_for_match(&m.token).chars().count() as isize;
            let norm_name_chars = normalize_for_match(&m.matched_name).chars().count() as isize;
            let len_diff = (norm_token_chars - norm_name_chars).abs() as f64;
            let proximity_factor = 1.0 / (1.0 + len_diff * 0.05);
            w_score *= proximity_factor;
        }

        m.weighted_score = w_score;
        grouped.entry(m.vndb_id.clone()).or_default().push(m);
    }

    // Overlapping Title Penalty (Cross-Group)
    // Create a vector of all structured matches by flattening the map values
    let mut all_structured_matches: Vec<MatchResult> =
        grouped.values().flatten().cloned().collect();

    for i in 0..all_structured_matches.len() {
        if all_structured_matches[i].match_method == MatchMethod::NameInToken {
            let norm_short_name = normalize_for_match(&all_structured_matches[i].matched_name);

            for j in 0..all_structured_matches.len() {
                if all_structured_matches[i].vndb_id != all_structured_matches[j].vndb_id {
                    let norm_long_name =
                        normalize_for_match(&all_structured_matches[j].matched_name);
                    if norm_long_name.contains(&norm_short_name)
                        && norm_long_name.chars().count() > norm_short_name.chars().count()
                    {
                        all_structured_matches[i].weighted_score *= 0.3;
                    }
                }
            }
        }
    }

    // Update grouped matches with penalized scores
    grouped.clear();
    for m in all_structured_matches {
        grouped.entry(m.vndb_id.clone()).or_default().push(m);
    }

    let mut candidates = Vec::new();

    for (vndb_id, game_matches) in grouped {
        let best_match = game_matches
            .iter()
            .max_by(|a, b| {
                a.weighted_score
                    .partial_cmp(&b.weighted_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap()
            .clone();

        let has_title_match = game_matches.iter().any(|m| {
            let base_type = m.match_type.split(':').next().unwrap_or(&m.match_type);
            base_type.starts_with("title") || base_type == "alias"
        });

        let has_brand_match = game_matches.iter().any(|m| {
            let base_type = m.match_type.split(':').next().unwrap_or(&m.match_type);
            base_type.starts_with("brand")
        });

        let cross_bonus = if has_title_match && has_brand_match {
            0.1
        } else {
            0.0
        };
        let final_score = best_match.weighted_score + cross_bonus;

        candidates.push(ScoredCandidate {
            vndb_id,
            final_score,
            best_match,
            all_matches: game_matches,
        });
    }

    candidates.sort_by(|a, b| {
        b.final_score
            .partial_cmp(&a.final_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                let a_len = a.best_match.matched_name.chars().count();
                let b_len = b.best_match.matched_name.chars().count();
                b_len.cmp(&a_len)
            })
    });

    let valid_candidates: Vec<ScoredCandidate> = candidates
        .into_iter()
        .filter(|c| c.final_score >= 0.4)
        .collect();

    if valid_candidates.is_empty() {
        None
    } else {
        Some(valid_candidates)
    }
}

pub fn score_and_select(matches: Vec<MatchResult>) -> Option<ScoredCandidate> {
    score_and_select_all(matches).and_then(|mut v| {
        if v.is_empty() {
            None
        } else {
            Some(v.remove(0))
        }
    })
}

#[cfg(test)]
mod tests {

    // ... unit tests can be added here
}
