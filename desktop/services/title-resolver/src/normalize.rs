use unicode_normalization::UnicodeNormalization;
use regex::Regex;
use lazy_static::lazy_static;

lazy_static! {
    static ref RE_NON_WORD: Regex = Regex::new(r"[^\w\s]").unwrap();
}

pub fn normalize_for_match(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    
    // 1. Lowercase
    let text = text.to_lowercase();
    
    // 2. Fullwidth to Halfwidth (NFKC)
    let text: String = text.nfkc().collect();
    
    // 3. Remove symbols
    let text = RE_NON_WORD.replace_all(&text, " ").to_string();
    let text = text.replace('_', " ");
    
    // 4. Space normalization
    let words: Vec<&str> = text.split_whitespace().collect();
    words.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize() {
        assert_eq!(normalize_for_match("FuriKuru_Game"), "furikuru game");
        assert_eq!(normalize_for_match("Ｆｕｌｌ　Ｗｉｄｔｈ"), "full width");
        assert_eq!(normalize_for_match("流星ワールドアクター: GB"), "流星ワールドアクター gb");
    }
}
