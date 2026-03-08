use anyhow::{Context, Result};
use std::fs::File;
use std::io::copy;
use std::path::Path;

pub struct DictDownloader;

impl DictDownloader {
    pub async fn ensure_latest(dest: &Path) -> Result<()> {
        let url =
            "https://github.com/ryoha000/remoterg/releases/latest/download/vndb_titles.db.zst";
        tracing::info!("Downloading dictionary from {}", url);
        Self::download_and_extract(url, dest).await
    }

    async fn download_and_extract(url: &str, dest: &Path) -> Result<()> {
        let response = reqwest::get(url).await.context("Failed to download dict")?;

        if !response.status().is_success() {
            anyhow::bail!("Failed to download dict: HTTP {}", response.status());
        }

        let bytes = response
            .bytes()
            .await
            .context("Failed to read response bytes")?;

        let mut decoder = zstd::stream::read::Decoder::new(&bytes[..])
            .context("Failed to initialize zstd decoder")?;

        let mut out = File::create(dest).context("Failed to create destination file")?;

        copy(&mut decoder, &mut out).context("Failed to decompress and write")?;

        Ok(())
    }
}
