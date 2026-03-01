use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

const API_BASE: &str = "https://api.vndb.org/kana";
/// 1リクエストあたり最大取得件数
const MAX_RESULTS_PER_PAGE: u32 = 100;

/// VNDB キャラクター情報
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VndbCharacter {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub original: Option<String>,
    /// キャラクター画像URL
    #[serde(default)]
    pub image_url: Option<String>,
    /// キャラクター説明
    #[serde(default)]
    pub description: String,
}

/// VNDB API のキャラクターレスポンス用内部型
#[derive(Debug, Deserialize)]
struct CharacterApiResponse {
    results: Vec<CharacterApiEntry>,
    more: bool,
}

#[derive(Debug, Deserialize)]
struct CharacterApiEntry {
    id: String,
    name: String,
    #[serde(default)]
    original: Option<String>,
    #[serde(default)]
    image: Option<CharacterImage>,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CharacterImage {
    url: Option<String>,
}

/// VNDB API クライアント
#[derive(Clone)]
pub struct VndbClient {
    http: reqwest::Client,
}

impl VndbClient {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { http }
    }

    /// vndb_id からキャラクター一覧を取得（ページネーション対応）
    /// main / primary ロールのキャラのみ取得し、最大 max_count 件に制限
    pub async fn get_characters(
        &self,
        vndb_id: &str,
        max_count: usize,
    ) -> Result<Vec<VndbCharacter>> {
        let mut all_characters = Vec::new();
        let mut page = 1u32;

        loop {
            let request_body = serde_json::json!({
                "filters": ["vn", "=", ["id", "=", vndb_id]],
                "fields": "name, original, image.url, description",
                "sort": "name",
                "results": MAX_RESULTS_PER_PAGE,
                "page": page
            });

            debug!(
                "VNDB API リクエスト: vndb_id={}, page={}",
                vndb_id, page
            );

            let response = self
                .http
                .post(format!("{}/character", API_BASE))
                .header("Content-Type", "application/json")
                .json(&request_body)
                .send()
                .await
                .context("VNDB API へのリクエスト送信に失敗")?;

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                anyhow::bail!(
                    "VNDB API エラー: status={}, body={}",
                    status,
                    body
                );
            }

            let api_response: CharacterApiResponse = response
                .json()
                .await
                .context("VNDB API レスポンスのパースに失敗")?;

            for entry in &api_response.results {
                all_characters.push(VndbCharacter {
                    id: entry.id.clone(),
                    name: entry.name.clone(),
                    original: entry.original.clone(),
                    image_url: entry.image.as_ref().and_then(|img| img.url.clone()),
                    description: entry.description.clone().unwrap_or_default(),
                });

                if all_characters.len() >= max_count {
                    break;
                }
            }

            info!(
                "VNDB キャラクター取得: page={}, 件数={}, 累計={}",
                page,
                api_response.results.len(),
                all_characters.len()
            );

            if !api_response.more || all_characters.len() >= max_count {
                break;
            }

            page += 1;
        }

        // max_count で切り詰め
        all_characters.truncate(max_count);

        info!(
            "VNDB キャラクター取得完了: vndb_id={}, 合計={}件",
            vndb_id,
            all_characters.len()
        );

        Ok(all_characters)
    }

    /// キャラクター画像をダウンロード
    /// 成功した場合は (キャラ表示名, 画像データ) を返す
    pub async fn download_character_images(
        &self,
        characters: &[VndbCharacter],
        max_images: usize,
        characters_dir: &std::path::Path,
        vndb_id: &str,
    ) -> Vec<(String, Vec<u8>)> {
        let mut images = Vec::new();

        let save_dir = characters_dir.join(vndb_id);
        if save_dir.exists() {
            if let Ok(mut entries) = tokio::fs::read_dir(&save_dir).await {
                let mut load_success = false;
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let path = entry.path();
                    if path.is_file() {
                        if let Some(name_os) = path.file_stem() {
                            if let Some(name) = name_os.to_str() {
                                if let Ok(bytes) = tokio::fs::read(&path).await {
                                    images.push((name.to_string(), bytes));
                                    load_success = true;
                                }
                            }
                        }
                    }
                }
                if load_success && !images.is_empty() {
                    info!("キャラ画像をキャッシュから{}枚読み込みました (vndb_id={})", images.len(), vndb_id);
                    // もし max_images より多ければ切り詰める（任意）
                    images.truncate(max_images);
                    return images;
                }
            }
        } else {
            let _ = tokio::fs::create_dir_all(&save_dir).await;
        }

        for character in characters.iter().take(max_images) {
            let image_url = match &character.image_url {
                Some(url) => url,
                None => continue,
            };

            // 表示名: 日本語名があればそちらを優先
            let display_name = character
                .original
                .as_deref()
                .unwrap_or(&character.name)
                .to_string();

            match self.http.get(image_url).send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        match response.bytes().await {
                            Ok(bytes) => {
                                debug!(
                                    "キャラ画像ダウンロード成功: {} ({} bytes)",
                                    display_name,
                                    bytes.len()
                                );
                                let data = bytes.to_vec();
                                images.push((display_name.clone(), data.clone()));
                                
                                // ファイル名に使用できない文字を置換
                                let safe_name = display_name.replace(|c: char| {
                                    matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
                                }, "_");
                                let file_path = save_dir.join(format!("{}.jpg", safe_name));
                                if let Err(e) = tokio::fs::write(&file_path, &data).await {
                                    warn!("画像の保存に失敗しました {}: {}", safe_name, e);
                                }
                            }
                            Err(e) => {
                                warn!(
                                    "キャラ画像のボディ読み取り失敗: {}: {}",
                                    display_name, e
                                );
                            }
                        }
                    } else {
                        warn!(
                            "キャラ画像ダウンロード失敗: {} status={}",
                            display_name,
                            response.status()
                        );
                    }
                }
                Err(e) => {
                    warn!(
                        "キャラ画像ダウンロードリクエスト失敗: {}: {}",
                        display_name, e
                    );
                }
            }
        }

        info!(
            "キャラ画像ダウンロード完了: {}/{}枚",
            images.len(),
            characters.iter().take(max_images).filter(|c| c.image_url.is_some()).count()
        );

        images
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // VNDB API を実際に叩くため通常テストでは除外
    async fn test_get_characters() {
        let client = VndbClient::new();
        // 流星ワールドアクター (v60196) のキャラクター一覧を取得
        let characters = client.get_characters("v60196", 50).await.unwrap();
        println!("取得したキャラクター数: {}", characters.len());
        for c in &characters {
            println!(
                "  {} / {} (image: {})",
                c.name,
                c.original.as_deref().unwrap_or("N/A"),
                c.image_url.as_deref().unwrap_or("N/A")
            );
        }
        assert!(!characters.is_empty(), "キャラクターが取得できること");
    }

    #[tokio::test]
    #[ignore]
    async fn test_download_character_images() {
        let client = VndbClient::new();
        let characters = client.get_characters("v60196", 8).await.unwrap();
        let temp_dir = std::env::temp_dir().join("remoterg_test_chars");
        let images = client.download_character_images(&characters, 4, &temp_dir, "v60196").await;
        println!("ダウンロードした画像数: {}", images.len());
        for (name, data) in &images {
            println!("  {} ({} bytes)", name, data.len());
        }
        let _ = tokio::fs::remove_dir_all(temp_dir).await;
    }
}
