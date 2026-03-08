use lazy_static::lazy_static;
use regex::Regex;
use std::collections::HashSet;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TokenRule {
    Original,
    RuleA,
    RuleB,
    RuleC,
    RuleD,
    RuleE,
}

const GENERIC_DIR_NAMES: &[&str] = &[
    "game",
    "games",
    "program files",
    "program files (x86)",
    "eroge",
    "visual novel",
    "vn",
    "users",
    "desktop",
    "download",
    "bin",
    "lib",
    "data",
];

const ENGINE_NAMES: &[&str] = &[
    "siglusengine",
    "bgi",
    "ethornell",
    "kirikiri",
    "rio",
    "alpharomdis",
    "catsystem2",
    "yu-ris",
    "majiro",
    "advhd",
    "qlie",
];

const UTILITY_FILENAMES: &[&str] = &[
    "setup",
    "install",
    "uninst",
    "uninstall",
    "patch",
    "update",
    "launcher",
    "config",
    "readme",
    "main",
];

const GENERIC_SUBWORDS: &[&str] = &["game", "app", "launcher"];

lazy_static! {
    static ref RE_DRIVE_LETTER: Regex = Regex::new(r"^[a-zA-Z]:$").unwrap();
    static ref RE_SEPARATOR: Regex = Regex::new(r"[_\-－―～~]").unwrap();
    static ref RE_SEPARATOR_EXTRACT: Regex = Regex::new(r"([_\-－―～~]+)").unwrap();
    static ref RE_CAMEL_1: Regex = Regex::new(r"([a-z])([A-Z])").unwrap();
    static ref RE_CAMEL_2: Regex = Regex::new(r"([A-Z])([A-Z][a-z])").unwrap();
    static ref RE_TYPE_1: Regex = Regex::new(r"([^\x00-\x7F])([a-zA-Z0-9])").unwrap();
    static ref RE_TYPE_2: Regex = Regex::new(r"([a-zA-Z0-9])([^\x00-\x7F])").unwrap();
    static ref RE_TYPE_3: Regex = Regex::new(r"([a-zA-Z])([0-9])").unwrap();
    static ref RE_TYPE_4: Regex = Regex::new(r"([0-9])([a-zA-Z])").unwrap();
}

/// パスをセグメントに分解し、不要なセグメントを除外する
pub fn extract_segments(process_path: &str) -> Vec<String> {
    let path = process_path.replace("\\", "/");
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();

    let mut segments = Vec::new();
    let len = parts.len();

    for (i, part) in parts.into_iter().enumerate() {
        if i == 0 && RE_DRIVE_LETTER.is_match(part) {
            continue;
        }

        let part_lower = part.to_lowercase();

        if GENERIC_DIR_NAMES.contains(&part_lower.as_str()) {
            continue;
        }

        if i == len - 1 {
            let path_obj = Path::new(part);
            if let Some(name) = path_obj.file_stem() {
                let name_lower = name.to_string_lossy().to_lowercase();
                if ENGINE_NAMES.contains(&name_lower.as_str())
                    || UTILITY_FILENAMES.contains(&name_lower.as_str())
                {
                    continue;
                }
            }
        }

        segments.push(part.to_string());
    }

    segments
}

/// Token Generation
pub fn generate_tokens(segments: &[String]) -> Vec<(String, TokenRule)> {
    let mut all_tokens = HashSet::new();

    for segment in segments {
        let mut tokens_for_segment = HashSet::new();
        tokens_for_segment.insert((segment.clone(), TokenRule::Original));

        let path_obj = Path::new(segment);
        if let Some(ext_os_str) = path_obj.extension() {
            let ext = ext_os_str.to_string_lossy().to_lowercase();
            if ["exe", "bin", "log", "bat", "lnk"].contains(&ext.as_str()) {
                if let Some(name_os_str) = path_obj.file_stem() {
                    let current_segment = name_os_str.to_string_lossy().to_string();
                    tokens_for_segment.insert((current_segment.clone(), TokenRule::RuleA));
                    if tokens_for_segment.contains(&(segment.clone(), TokenRule::Original))
                        && segment != &current_segment
                    {
                        tokens_for_segment.remove(&(segment.clone(), TokenRule::Original));
                    }
                }
            }
        }

        let mut new_tokens = HashSet::new();
        for (t, _rule) in &tokens_for_segment {
            // Rule B
            if RE_SEPARATOR.is_match(t) {
                let spaced = RE_SEPARATOR_EXTRACT.replace_all(t, " ").trim().to_string();
                if !spaced.is_empty() {
                    new_tokens.insert((spaced, TokenRule::RuleB));
                }

                if let Some(mat) = RE_SEPARATOR_EXTRACT.find(t) {
                    let prefix = t[..mat.start()].trim();
                    if !prefix.is_empty() {
                        new_tokens.insert((prefix.to_string(), TokenRule::RuleB));
                    }
                }
            }

            // Rule C
            let mut camel_split = RE_CAMEL_1.replace_all(t, "$1 $2").to_string();
            camel_split = RE_CAMEL_2.replace_all(&camel_split, "$1 $2").to_string();
            if camel_split != *t {
                new_tokens.insert((camel_split, TokenRule::RuleC));
            }

            // Rule D
            let mut type_split = RE_TYPE_1.replace_all(t, "$1 $2").to_string();
            type_split = RE_TYPE_2.replace_all(&type_split, "$1 $2").to_string();
            type_split = RE_TYPE_3.replace_all(&type_split, "$1 $2").to_string();
            type_split = RE_TYPE_4.replace_all(&type_split, "$1 $2").to_string();

            if type_split != *t {
                new_tokens.insert((type_split, TokenRule::RuleD));
            }
        }
        tokens_for_segment.extend(new_tokens);

        // Rule E
        let mut final_new_tokens = HashSet::new();
        for (t, _rule) in &tokens_for_segment {
            let words: Vec<&str> = t.split_whitespace().collect();
            if words.len() > 1 {
                let filtered_words: Vec<&str> = words
                    .iter()
                    .copied()
                    .filter(|w| !GENERIC_SUBWORDS.contains(&w.to_lowercase().as_str()))
                    .collect();
                if !filtered_words.is_empty() && filtered_words.len() < words.len() {
                    final_new_tokens.insert((filtered_words.join(" "), TokenRule::RuleE));
                }
            }
        }

        tokens_for_segment.extend(final_new_tokens);
        all_tokens.extend(tokens_for_segment);
    }

    let mut list: Vec<(String, TokenRule)> = all_tokens.into_iter().collect();
    list.sort();
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_segments_case1() {
        let segments =
            extract_segments(r"G:\game\Heliodor\流星ワールドアクターGB\WorldActorGB.exe");
        assert_eq!(
            segments,
            vec!["Heliodor", "流星ワールドアクターGB", "WorldActorGB.exe"]
        );
    }

    #[test]
    fn test_generate_tokens_case1() {
        let segments = vec!["流星ワールドアクターGB".to_string()];
        let tokens = generate_tokens(&segments);
        assert!(tokens.contains(&("流星ワールドアクター GB".to_string(), TokenRule::RuleD)));
    }
}
