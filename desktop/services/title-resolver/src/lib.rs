pub mod normalize;
pub mod tokenizer;
pub mod matcher;
pub mod scorer;
pub mod downloader;

pub use matcher::{MatchResult, match_token};
pub use scorer::{ScoredCandidate, score_and_select};
pub use downloader::DictDownloader;

use rusqlite::Connection;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct TitleResolveResult {
    pub vndb_id: String,
    pub official_title: String,
    pub confidence: f64,
}
pub struct DictEntry {
    pub normalized_name: String,
    pub no_space_name: String,
    pub vndb_id: String,
    pub match_type: String,
    pub original_name: String,
}

pub struct TitleResolver {
    entries: Vec<DictEntry>,
    games: std::collections::HashMap<String, String>,
}

impl TitleResolver {
    pub fn new(db_path: &Path) -> rusqlite::Result<Self> {
        let db = Connection::open(db_path)?;
        
        let mut entries = Vec::new();
        let mut stmt = db.prepare("SELECT normalized_name, no_space_name, game_id, match_type, original_name FROM dict_entries")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            entries.push(DictEntry {
                normalized_name: row.get(0)?,
                no_space_name: row.get(1)?,
                vndb_id: row.get(2)?,
                match_type: row.get(3)?,
                original_name: row.get(4)?,
            });
        }
        
        let mut games = std::collections::HashMap::new();
        // SQLite カラム名 game_id はそのまま維持（互換性のため）
        let mut stmt = db.prepare("SELECT game_id, official_title FROM games")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let title: String = row.get(1)?;
            games.insert(id, title);
        }
        
        Ok(Self { entries, games })
    }

    pub fn resolve(&self, process_path: &str) -> Option<TitleResolveResult> {
        let segments = tokenizer::extract_segments(process_path);
        let tokens = tokenizer::generate_tokens(&segments);
        
        let mut all_matches = Vec::new();
        for (token, rule) in tokens {
            let mut matches = matcher::match_token(&self.entries, &token, &rule);
            all_matches.append(&mut matches);
        }
        
        let mut top_candidates = Vec::new();
        if let Some(candidates) = scorer::score_and_select_all(all_matches) {
            top_candidates = candidates;
        }
        
        if top_candidates.is_empty() { return None; }
        let candidate = &top_candidates[0];
        
        let official_title = self.get_official_title(&candidate.vndb_id)
            .unwrap_or_else(|| "Unknown".to_string());
            
        Some(TitleResolveResult {
            vndb_id: candidate.vndb_id.clone(),
            official_title,
            confidence: candidate.final_score,
        })
    }
    
    // For testing and debug
    pub fn resolve_all(&self, process_path: &str) -> Vec<(TitleResolveResult, Vec<scorer::ScoredCandidate>)> {
        let segments = tokenizer::extract_segments(process_path);
        let tokens = tokenizer::generate_tokens(&segments);
        
        let mut all_matches = Vec::new();
        for (token, rule) in tokens {
            let mut matches = matcher::match_token(&self.entries, &token, &rule);
            all_matches.append(&mut matches);
        }
        
        if let Some(candidates) = scorer::score_and_select_all(all_matches) {
            let res = candidates.iter().map(|c| {
                let official_title = self.get_official_title(&c.vndb_id)
                    .unwrap_or_else(|| String::new());
                let result = TitleResolveResult {
                    vndb_id: c.vndb_id.clone(),
                    official_title,
                    confidence: c.final_score,
                };
                (result, vec![c.clone()])
            }).collect();
            return res;
        }
        Vec::new()
    }
    
    fn get_official_title(&self, vndb_id: &str) -> Option<String> {
        self.games.get(vndb_id).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_resolve_with_real_db() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let db_path = PathBuf::from(manifest_dir).join("../../../scripts/vndb_titles.db");

        if !db_path.exists() {
            println!("Skipping test_resolve_with_real_db because db does not exist at {:?}", db_path);
            return;
        }

        let resolver = TitleResolver::new(&db_path).unwrap();

        let cases = vec![
            (
                r"G:\game\Heliodor\流星ワールドアクターGB\WorldActorGB.exe",
                "v60196",
                "流星ワールドアクター Gaslight Bullet",
            ),
            (
                r"C:\Program Files\NINJA GIRL\ninjagirl.exe",
                "v21435",
                "Ninja Girl and the Mysterious Army of Urban Legend Monsters! ~Hunt of the Headless Horseman~",
            ),
            (
                r"D:\Games\AQUAPLUS\WHITE ALBUM2 Extended Edition\WA2.exe",
                "v7771",
                "WHITE ALBUM2",
            ),
            (
                r"F:\Games\9-nine-ここのつここのかここのいろ\9-nine-kokono.exe",
                "v19829",
                "9-nine-ここのつここのかここのいろ",
            )
        ];

        for (path, expected_id, expected_title) in cases {
            let result_all = resolver.resolve_all(path);
            for r in &result_all {
                println!("  Candidate: {} (score: {})", r.0.official_title, r.0.confidence);
                let c = &r.1[0];
                println!("    Best Match: Token '{}' matched '{}' (Method: {:?}, Score: {})", c.best_match.token, c.best_match.matched_name, c.best_match.match_method, c.best_match.weighted_score);
            }
            let result = resolver.resolve(path);
            assert!(result.is_some(), "Failed to resolve path: {}", path);
            let result = result.unwrap();
            assert_eq!(result.vndb_id, expected_id, "Path failed: {}", path);
            assert_eq!(result.official_title, expected_title, "Path failed: {}", path);
            println!("Resolved {} -> {} (score: {})", path, result.official_title, result.confidence);
        }
    }
}
