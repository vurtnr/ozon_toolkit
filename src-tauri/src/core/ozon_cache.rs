use std::io::Cursor;
use std::path::{Path, PathBuf};

use image::ImageFormat;
use reqwest::Url;
use serde::{Deserialize, Serialize};

use crate::core::ozon_product::OzonProductResolution;

const CACHE_ENTRY_META_FILE: &str = "meta.json";
const CACHE_ENTRY_IMAGE_FILE: &str = "source.png";
const MIN_OZON_SOURCE_IMAGE_DIMENSION: u32 = 120;

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
    source_key: String,
    title: String,
    image_url: String,
    #[serde(default)]
    spec_profile: crate::core::ozon_product::OzonSpecProfile,
}

pub fn validate_ozon_source_metadata(title: &str, image_url: &str) -> Result<(), String> {
    let normalized_title = title.trim().to_lowercase();
    let normalized_image_url = image_url.trim().to_lowercase();
    let title_looks_generic = ["купить на ozon", "цена на ozon", "доставка на ozon"]
        .into_iter()
        .any(|hint| normalized_title.contains(hint));
    let image_looks_generic = ["og_ozon_ru.png", "/s3/cms/logo/", "/cms/logo/"]
        .into_iter()
        .any(|hint| normalized_image_url.contains(hint));

    if title_looks_generic || image_looks_generic {
        return Err("generic ozon listing metadata".to_string());
    }

    Ok(())
}

fn validate_ozon_cache_metadata(metadata: &OzonCacheMetadata) -> Result<(), String> {
    validate_ozon_source_metadata(&metadata.title, &metadata.image_url)
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

    pub fn lookup(&self, source_key: &str) -> Result<OzonSourceCacheLookup, String> {
        let entry_dir = self.entry_dir(source_key)?;
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
        let normalized_source_key = normalize_cache_source_key(source_key)?;
        if metadata.source_key != normalized_source_key {
            return Ok(OzonSourceCacheLookup::Corrupted(
                "ozon cache metadata key mismatch".to_string(),
            ));
        }
        if let Err(error) = validate_ozon_cache_metadata(&metadata) {
            return Ok(OzonSourceCacheLookup::Corrupted(error));
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
        if let Err(error) = validate_ozon_source_image_bytes(&image_bytes) {
            return Ok(OzonSourceCacheLookup::Corrupted(error));
        }

        Ok(OzonSourceCacheLookup::Hit(OzonProductResolution {
            title: metadata.title,
            image_url: metadata.image_url,
            image_bytes,
            spec_profile: metadata.spec_profile,
        }))
    }

    pub fn store(
        &self,
        source_key: &str,
        resolution: &OzonProductResolution,
    ) -> Result<(), String> {
        let entry_dir = self.entry_dir(source_key)?;
        std::fs::create_dir_all(&entry_dir)
            .map_err(|e| format!("create ozon cache entry dir failed: {e}"))?;

        let metadata = OzonCacheMetadata {
            source_key: normalize_cache_source_key(source_key)?,
            title: resolution.title.clone(),
            image_url: resolution.image_url.clone(),
            spec_profile: resolution.spec_profile.clone(),
        };
        validate_ozon_cache_metadata(&metadata)?;
        let metadata_bytes = serde_json::to_vec_pretty(&metadata)
            .map_err(|e| format!("serialize ozon cache metadata failed: {e}"))?;
        let image_bytes = normalize_cache_image_bytes(&resolution.image_bytes)?;

        std::fs::write(entry_dir.join(CACHE_ENTRY_META_FILE), metadata_bytes)
            .map_err(|e| format!("write ozon cache metadata failed: {e}"))?;
        std::fs::write(entry_dir.join(CACHE_ENTRY_IMAGE_FILE), image_bytes)
            .map_err(|e| format!("write ozon cache image failed: {e}"))?;
        Ok(())
    }

    fn entry_dir(&self, source_key: &str) -> Result<PathBuf, String> {
        let normalized_source_key = normalize_cache_source_key(source_key)?;
        Ok(self.root_dir.join(cache_key_for_source(&normalized_source_key)))
    }
}

fn normalize_cache_source_key(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("normalize ozon cache key failed: empty source key".to_string());
    }

    if let Ok(mut url) = Url::parse(trimmed) {
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
        return Ok(url.to_string());
    }

    Ok(trimmed.to_string())
}

fn cache_key_for_source(normalized_source_key: &str) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in normalized_source_key.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn normalize_cache_image_bytes(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let dynamic = decode_ozon_source_image(bytes)?;
    validate_decoded_ozon_source_image(&dynamic)?;
    let mut cursor = Cursor::new(Vec::new());
    dynamic
        .write_to(&mut cursor, ImageFormat::Png)
        .map_err(|e| format!("encode ozon cache image failed: {e}"))?;
    Ok(cursor.into_inner())
}

pub fn validate_ozon_source_image_bytes(bytes: &[u8]) -> Result<(), String> {
    let dynamic = decode_ozon_source_image(bytes)?;
    validate_decoded_ozon_source_image(&dynamic)
}

fn decode_ozon_source_image(bytes: &[u8]) -> Result<image::DynamicImage, String> {
    image::load_from_memory(bytes).map_err(|e| format!("decode ozon source image failed: {e}"))
}

fn validate_decoded_ozon_source_image(dynamic: &image::DynamicImage) -> Result<(), String> {
    let width = dynamic.width();
    let height = dynamic.height();
    if width < MIN_OZON_SOURCE_IMAGE_DIMENSION || height < MIN_OZON_SOURCE_IMAGE_DIMENSION {
        return Err(format!(
            "ozon source image too small: {}x{}",
            width, height
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageBuffer, Rgba};

    fn build_png(width: u32, height: u32) -> Vec<u8> {
        let image = DynamicImage::ImageRgba8(ImageBuffer::from_pixel(
            width,
            height,
            Rgba([12, 34, 56, 255]),
        ));
        let mut cursor = Cursor::new(Vec::new());
        image
            .write_to(&mut cursor, ImageFormat::Png)
            .expect("png should encode");
        cursor.into_inner()
    }

    #[test]
    fn lookup_marks_tiny_cached_image_as_corrupted() {
        let cache_root = std::env::temp_dir().join(format!(
            "ozon-cache-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time should move forward")
                .as_nanos()
        ));
        let cache = OzonSourceCache::new(cache_root.clone());
        let source_key = "https://www.ozon.ru/product/3570411009/";
        let entry_dir = cache
            .entry_dir(source_key)
            .expect("cache entry dir should resolve");
        std::fs::create_dir_all(&entry_dir).expect("cache entry dir should exist");
        std::fs::write(
            entry_dir.join(CACHE_ENTRY_META_FILE),
            serde_json::to_vec_pretty(&OzonCacheMetadata {
                source_key: normalize_cache_source_key(source_key)
                    .expect("source key should normalize"),
                title: "Tiny QR".to_string(),
                image_url: "https://ir.ozone.ru/s3/multimedia-1-7/wc800/8908721791.jpg"
                    .to_string(),
                spec_profile: crate::core::ozon_product::OzonSpecProfile::default(),
            })
            .expect("metadata should serialize"),
        )
        .expect("metadata should write");
        std::fs::write(entry_dir.join(CACHE_ENTRY_IMAGE_FILE), build_png(68, 68))
            .expect("image should write");

        let lookup = cache.lookup(source_key).expect("lookup should succeed");

        let _ = std::fs::remove_dir_all(cache_root);

        assert!(
            matches!(lookup, OzonSourceCacheLookup::Corrupted(_)),
            "tiny cached images should be invalidated instead of reused"
        );
    }

    #[test]
    fn lookup_marks_generic_ozon_listing_cache_as_corrupted() {
        let cache_root = std::env::temp_dir().join(format!(
            "ozon-cache-generic-listing-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time should move forward")
                .as_nanos()
        ));
        let cache = OzonSourceCache::new(cache_root.clone());
        let source_key = "https://www.ozon.ru/product/3560192694/";
        let entry_dir = cache
            .entry_dir(source_key)
            .expect("cache entry dir should resolve");
        std::fs::create_dir_all(&entry_dir).expect("cache entry dir should exist");
        std::fs::write(
            entry_dir.join(CACHE_ENTRY_META_FILE),
            serde_json::to_vec_pretty(&OzonCacheMetadata {
                source_key: normalize_cache_source_key(source_key)
                    .expect("source key should normalize"),
                title: "Чехол для планшета - купить на OZON".to_string(),
                image_url: "https://ir.ozone.ru/s3/cms/logo/og_ozon_ru.png".to_string(),
                spec_profile: crate::core::ozon_product::OzonSpecProfile::default(),
            })
            .expect("metadata should serialize"),
        )
        .expect("metadata should write");
        std::fs::write(entry_dir.join(CACHE_ENTRY_IMAGE_FILE), build_png(320, 320))
            .expect("image should write");

        let lookup = cache.lookup(source_key).expect("lookup should succeed");

        let _ = std::fs::remove_dir_all(cache_root);

        match lookup {
            OzonSourceCacheLookup::Corrupted(error) => {
                assert!(
                    error.contains("generic ozon listing metadata"),
                    "unexpected corruption error: {error}"
                );
            }
            other => panic!("expected generic listing cache entry to be invalidated, got {other:?}"),
        }
    }
}
