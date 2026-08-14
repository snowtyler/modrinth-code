use crate::event::LoadingBarType;
use crate::event::emit::{emit_loading, init_loading};
use crate::util::fetch::REQWEST_CLIENT;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
use tokio::io::AsyncWriteExt;
use tracing::{info, warn};

pub const SEED_CACHE_CONFIG_NAME: &str = "seed-cache.json";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SeedCacheConfig {
    #[serde(default)]
    pub caches: Vec<SeedCacheEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedCacheEntry {
    pub name: String,
    pub url: String,
    pub target_path: String,
    #[serde(default = "default_marker_file")]
    pub marker_file: String,
    pub sha256: Option<String>,
}

fn default_marker_file() -> String {
    "CURRENT".to_string()
}

/// Checks and seeds any caches specified in `seed-cache.json` or `config/seed-cache.json`.
/// Uses a fast-path check against `marker_file` so already-seeded caches return in < 1ms.
#[tracing::instrument(skip(instance_dir))]
pub async fn ensure_instance_caches_seeded(instance_dir: &Path) -> crate::Result<()> {
    let config_path_direct = instance_dir.join(SEED_CACHE_CONFIG_NAME);
    let config_path_in_config = instance_dir.join("config").join(SEED_CACHE_CONFIG_NAME);

    let config_file = if config_path_direct.is_file() {
        Some(config_path_direct)
    } else if config_path_in_config.is_file() {
        Some(config_path_in_config)
    } else {
        None
    };

    let Some(config_file) = config_file else {
        return Ok(());
    };

    let content = match tokio::fs::read_to_string(&config_file).await {
        Ok(c) => c,
        Err(err) => {
            warn!("Failed to read {}: {err}", config_file.display());
            return Ok(());
        }
    };

    let config: SeedCacheConfig = match serde_json::from_str(&content) {
        Ok(cfg) => cfg,
        Err(err) => {
            warn!("Failed to parse {}: {err}", config_file.display());
            return Ok(());
        }
    };

    for entry in &config.caches {
        seed_cache_entry(instance_dir, entry).await?;
    }

    Ok(())
}

async fn seed_cache_entry(instance_dir: &Path, entry: &SeedCacheEntry) -> crate::Result<()> {
    let target_dir = instance_dir.join(&entry.target_path);
    let marker_path = target_dir.join(&entry.marker_file);

    // Fast-path: if marker exists, cache is fully seeded
    if marker_path.exists() {
        tracing::debug!(
            name = %entry.name,
            target = %target_dir.display(),
            "Cache already seeded, skipping"
        );
        return Ok(());
    }

    info!(
        name = %entry.name,
        url = %entry.url,
        target = %target_dir.display(),
        "Downloading cache archive"
    );

    crate::util::io::create_dir_all(&target_dir).await?;

    let temp_download_path = target_dir.join(".tmp_download");
    let loading_key = init_loading(
        LoadingBarType::ZipExtract {
            instance_id: String::new(),
            instance_name: entry.name.clone(),
        },
        1.0,
        &format!("Downloading {}", entry.name),
    )
    .await?;

    let response = REQWEST_CLIENT.get(&entry.url).send().await.map_err(|err| {
        crate::ErrorKind::OtherError(format!("Failed to fetch cache from {}: {err}", entry.url))
    })?;

    if !response.status().is_success() {
        return Err(crate::ErrorKind::OtherError(format!(
            "Failed to download cache from {} with status {}",
            entry.url,
            response.status()
        ))
        .into());
    }

    let total_size = response.content_length();
    let mut stream = response.bytes_stream();
    let mut file = tokio::fs::File::create(&temp_download_path).await?;
    let mut hasher = Sha256::new();
    let mut downloaded: u64 = 0;

    while let Some(chunk_res) = stream.next().await {
        let chunk = chunk_res.map_err(|err| {
            crate::ErrorKind::OtherError(format!("Error while streaming cache download: {err}"))
        })?;

        file.write_all(&chunk).await?;
        hasher.update(&chunk);
        downloaded += chunk.len() as u64;

        if let Some(total) = total_size {
            if total > 0 {
                let progress = (downloaded as f64) / (total as f64);
                let current_mb = (downloaded as f64) / (1024.0 * 1024.0);
                let total_mb = (total as f64) / (1024.0 * 1024.0);
                let _ = emit_loading(
                    &loading_key,
                    progress * 0.7,
                    Some(&format!(
                        "Downloading {} ({:.1} MB / {:.1} MB)",
                        entry.name, current_mb, total_mb
                    )),
                );
            }
        }
    }

    file.flush().await?;
    drop(file);

    // Verify SHA256 if provided
    if let Some(expected_sha256) = &entry.sha256 {
        let calculated = format!("{:x}", hasher.finalize());
        if !calculated.eq_ignore_ascii_case(expected_sha256.trim()) {
            let _ = tokio::fs::remove_file(&temp_download_path).await;
            return Err(crate::ErrorKind::OtherError(format!(
                "SHA256 checksum mismatch for cache {}: expected {expected_sha256}, got {calculated}",
                entry.name
            ))
            .into());
        }
    }

    let _ = emit_loading(
        &loading_key,
        0.8,
        Some(&format!("Extracting {}...", entry.name)),
    );

    // Atomic extraction in blocking task
    let temp_download_clone = temp_download_path.clone();
    let target_dir_clone = target_dir.clone();
    tokio::task::spawn_blocking(move || {
        extract_zip_archive(&temp_download_clone, &target_dir_clone)
    })
    .await
    .map_err(|err| crate::ErrorKind::OtherError(format!("Zip extraction task panicked: {err}")))?
    .map_err(|err| {
        crate::ErrorKind::OtherError(format!("Failed to extract cache archive: {err}"))
    })?;

    // Remove temporary download file
    let _ = tokio::fs::remove_file(&temp_download_path).await;

    // Write marker file LAST to guarantee atomicity
    let timestamp = chrono::Utc::now().to_rfc3339();
    tokio::fs::write(&marker_path, format!("SEEDED_AT={timestamp}\n")).await?;

    let _ = emit_loading(
        &loading_key,
        1.0,
        Some(&format!("Seeded {}", entry.name)),
    );

    info!(
        name = %entry.name,
        target = %target_dir.display(),
        "Successfully seeded cache"
    );

    Ok(())
}

fn extract_zip_archive(zip_path: &Path, target_dir: &Path) -> Result<(), String> {
    let file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;

    for i in 0..archive.len() {
        let mut zip_file = archive.by_index(i).map_err(|e| e.to_string())?;
        let relative_path = match zip_file.enclosed_name() {
            Some(path) => path.to_owned(),
            None => continue,
        };

        let out_path = target_dir.join(relative_path);

        if zip_file.is_dir() {
            std::fs::create_dir_all(&out_path).map_err(|e| e.to_string())?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut outfile = std::fs::File::create(&out_path).map_err(|e| e.to_string())?;
            std::io::copy(&mut zip_file, &mut outfile).map_err(|e| e.to_string())?;
        }
    }

    Ok(())
}
