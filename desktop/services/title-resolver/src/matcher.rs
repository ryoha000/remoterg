use crate::normalize::normalize_for_match;
use crate::tokenizer::TokenRule;
use crate::DictEntry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchMethod {
    Exact,
    ExactSpaceAgnostic,
    Prefix,
    TokenInName,
    NameInToken,
    TokenInNameSpaceAgnostic,
    NameInTokenSpaceAgnostic,
}

#[derive(Debug, Clone)]
pub struct MatchResult {
    pub game_id: String,
    pub match_type: String,
    pub match_method: MatchMethod,
    pub score: f64,
    pub token: String,
    pub matched_name: String,
    pub derived_rule: TokenRule,
    pub weighted_score: f64, // For step 4
}

pub fn match_token(entries: &[DictEntry], token: &str, derived_rule: &TokenRule) -> Vec<MatchResult> {
    let norm_token = normalize_for_match(token);
    if norm_token.is_empty() {
        return Vec::new();
    }
    
    let token_no_space = norm_token.replace(" ", "");
    
    let mut results = Vec::new();
    let norm_token_chars = norm_token.chars().count() as f64;
    let token_no_space_chars = token_no_space.chars().count() as f64;
    
    for entry in entries {
        let norm_name = &entry.normalized_name;
        let name_no_space = &entry.no_space_name;
        
        let mut method = None;
        let mut score = 0.0;
        
        if norm_token == *norm_name {
            method = Some(MatchMethod::Exact);
            score = 1.0;
        } else if token_no_space == *name_no_space && !token_no_space.is_empty() {
            method = Some(MatchMethod::ExactSpaceAgnostic);
            score = 0.95;
        } else if norm_token_chars >= 3.0 && norm_name.starts_with(&norm_token) {
            method = Some(MatchMethod::Prefix);
            let norm_name_chars = norm_name.chars().count() as f64;
            score = 0.85 * (norm_token_chars / norm_name_chars).powf(0.1);
        } else if norm_token_chars >= 3.0 && norm_name.contains(&norm_token) {
            method = Some(MatchMethod::TokenInName);
            let norm_name_chars = norm_name.chars().count() as f64;
            score = norm_token_chars / norm_name_chars;
        } else if norm_token.contains(norm_name.as_str()) {
            let norm_name_chars = norm_name.chars().count() as f64;
            if norm_name_chars >= 3.0 {
                method = Some(MatchMethod::NameInToken);
                score = norm_name_chars / norm_token_chars;
            }
        } else if token_no_space_chars >= 5.0 && name_no_space.contains(&token_no_space) {
            method = Some(MatchMethod::TokenInNameSpaceAgnostic);
            let name_no_space_chars = name_no_space.chars().count() as f64;
            score = 0.95 * (token_no_space_chars / name_no_space_chars);
        } else if token_no_space.contains(name_no_space.as_str()) {
            let name_no_space_chars = name_no_space.chars().count() as f64;
            if name_no_space_chars >= 5.0 {
                method = Some(MatchMethod::NameInTokenSpaceAgnostic);
                score = 0.95 * (name_no_space_chars / token_no_space_chars);
            }
        }
        
        if let Some(m) = method {
            results.push(MatchResult {
                game_id: entry.game_id.clone(),
                match_type: entry.match_type.clone(),
                match_method: m,
                score,
                token: token.to_string(),
                matched_name: entry.original_name.clone(),
                derived_rule: derived_rule.clone(),
                weighted_score: 0.0,
            });
        }
    }
    
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test_entries() -> Vec<DictEntry> {
        vec![
            DictEntry {
                normalized_name: "heliodor".to_string(),
                no_space_name: "heliodor".to_string(),
                game_id: "v1".to_string(),
                match_type: "brand".to_string(),
                original_name: "Heliodor".to_string(),
            },
            DictEntry {
                normalized_name: "流星ワールドアクター gaslight bullet".to_string(),
                no_space_name: "流星ワールドアクターgaslightbullet".to_string(),
                game_id: "v60196".to_string(),
                match_type: "title".to_string(),
                original_name: "流星ワールドアクター Gaslight Bullet".to_string(),
            }
        ]
    }

    #[test]
    fn test_exact_match() {
        let entries = setup_test_entries();
        let results = match_token(&entries, "Heliodor", &TokenRule::Original);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].match_method, MatchMethod::Exact);
        assert_eq!(results[0].score, 1.0);
    }

    #[test]
    fn test_token_in_name() {
        let entries = setup_test_entries();
        let results = match_token(&entries, "流星ワールドアクター", &TokenRule::RuleD);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].match_method, MatchMethod::Prefix);
        assert!(results[0].score < 1.0);
        assert_eq!(results[0].game_id, "v60196");
    }
}
