use std::io::Cursor;
use std::path::{Path, PathBuf};

use image::ImageFormat;
use reqwest::Url;
use serde::{Deserialize, Serialize};

use crate::core::ozon_product::OzonProductResolution;

const CACHE_ENTRY_META_FILE: &str = "meta.json";
const CACHE_ENTRY_IMAGE_FILE: &str = "source.png";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OzonSourceCacheLookup {
    Hit(OzonProductResolution),
    Miss,
    Corrupted(String),
}

#[derive(Debug, Clone)]
pub struct OzonSourceCache {
    root_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OzonCacheMetadata {
    canonical_url: String,
    title: String,
    image_url: String,
}

pub fn cache_root_for_output_anchor(output_anchor_path: &Path) -> PathBuf {
    output_anchor_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(".desktop_app_cache")
        .join("ozon_source")
}

impl OzonSourceCache {
    pub fn new(root_dir: PathBuf) -> Self {
        Self { root_dir }
    }

    pub fn for_output_anchor(output_anchor_path: &Path) -> Self {
        Self::new(cache_root_for_output_anchor(output_anchor_path))
    }

    pub fn lookup(&self, product_url: &str) -> Result<OzonSourceCacheLookup, String> {
        let entry_dir = self.entry_dir(product_url)?;
        if !entry_dir.exists() {
            return Ok(OzonSourceCacheLookup::Miss);
        }

        let metadata_path = entry_dir.join(CACHE_ENTRY_META_FILE);
        let image_path = entry_dir.join(CACHE_ENTRY_IMAGE_FILE);
        if !metadata_path.exists() || !image_path.exists() {
            return Ok(OzonSourceCacheLookup::Corrupted(
                "ozon cache entry is incomplete".to_string(),
            ));
        }

        let metadata_bytes = match std::fs::read(&metadata_path) {
            Ok(bytes) => bytes,
            Err(error) => {
                return Ok(OzonSourceCacheLookup::Corrupted(format!(
                    "read ozon cache metadata failed: {error}"
                )));
            }
        };
        let metadata = match serde_json::from_slice::<OzonCacheMetadata>(&metadata_bytes) {
            Ok(metadata) => metadata,
            Err(error) => {
                return Ok(OzonSourceCacheLookup::Corrupted(format!(
                    "parse ozon cache metadata failed: {error}"
                )));
            }
        };
        let canonical_url = canonicalize_product_url(product_url)?;
        if metadata.canonical_url != canonical_url {
            return Ok(OzonSourceCacheLookup::Corrupted(
                "ozon cache metadata url mismatch".to_string(),
            ));
        }

        let image_bytes = match std::fs::read(&image_path) {
            Ok(bytes) => bytes,
            Err(error) => {
                return Ok(OzonSourceCacheLookup::Corrupted(format!(
                    "read ozon cache image failed: {error}"
                )));
            }
        };
        if image_bytes.is_empty() {
            return Ok(OzonSourceCacheLookup::Corrupted(
                "ozon cache image is empty".to_string(),
            ));
        }

        Ok(OzonSourceCacheLookup::Hit(OzonProductResolution {
            title: metadata.title,
            image_url: metadata.image_url,
            image_bytes,
        }))
    }

    pub fn store(
        &self,
        product_url: &str,
        resolution: &OzonProductResolution,
    ) -> Result<(), String> {
        let entry_dir = self.entry_dir(product_url)?;
        std::fs::create_dir_all(&entry_dir)
            .map_err(|e| format!("create ozon cache entry dir failed: {e}"))?;

        let metadata = OzonCacheMetadata {
            canonical_url: canonicalize_product_url(product_url)?,
            title: resolution.title.clone(),
            image_url: resolution.image_url.clone(),
        };
        let metadata_bytes = serde_json::to_vec_pretty(&metadata)
            .map_err(|e| format!("serialize ozon cache metadata failed: {e}"))?;
        let image_bytes = normalize_cache_image_bytes(&resolution.image_bytes)?;

        std::fs::write(entry_dir.join(CACHE_ENTRY_META_FILE), metadata_bytes)
            .map_err(|e| format!("write ozon cache metadata failed: {e}"))?;
        std::fs::write(entry_dir.join(CACHE_ENTRY_IMAGE_FILE), image_bytes)
            .map_err(|e| format!("write ozon cache image failed: {e}"))?;
        Ok(())
    }

    fn entry_dir(&self, product_url: &str) -> Result<PathBuf, String> {
        let canonical_url = canonicalize_product_url(product_url)?;
        Ok(self.root_dir.join(cache_key_for_url(&canonical_url)))
    }
}

fn canonicalize_product_url(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("normalize ozon cache url failed: empty product url".to_string());
    }

    let mut url =
        Url::parse(trimmed).map_err(|e| format!("normalize ozon cache url failed: {e}"))?;
    if let Some(host) = url.host_str().map(|value| value.to_ascii_lowercase()) {
        url.set_host(Some(&host))
            .map_err(|_| "normalize ozon cache url failed: invalid host".to_string())?;
    }
    url.set_fragment(None);
    let path = url.path().trim_end_matches('/').to_string();
    if path.is_empty() {
        url.set_path("/");
    } else {
        url.set_path(&path);
    }
    Ok(url.to_string())
}

fn cache_key_for_url(canonical_url: &str) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in canonical_url.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn normalize_cache_image_bytes(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let dynamic = image::load_from_memory(bytes)
        .map_err(|e| format!("decode ozon cache image failed: {e}"))?;
    let mut cursor = Cursor::new(Vec::new());
    dynamic
        .write_to(&mut cursor, ImageFormat::Png)
        .map_err(|e| format!("encode ozon cache image failed: {e}"))?;
    Ok(cursor.into_inner())
}
