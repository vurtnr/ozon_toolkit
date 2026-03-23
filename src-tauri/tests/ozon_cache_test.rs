use desktop_app_lib::core::ozon_cache::{
    cache_root_for_output_anchor, OzonSourceCache, OzonSourceCacheLookup,
};
use desktop_app_lib::core::ozon_product::OzonProductResolution;
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use std::path::PathBuf;

fn make_temp_dir(name: &str) -> PathBuf {
    let unique = format!(
        "{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos()
    );
    std::env::temp_dir().join(unique)
}

fn sample_resolution() -> OzonProductResolution {
    let image = RgbaImage::from_fn(2, 2, |x, y| {
        if (x + y) % 2 == 0 {
            Rgba([220, 40, 40, 255])
        } else {
            Rgba([245, 245, 245, 255])
        }
    });
    let mut cursor = std::io::Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut cursor, ImageFormat::Png)
        .expect("sample png should encode");

    OzonProductResolution {
        title: "Морская верёвочная лестница".to_string(),
        image_url: "https://cdn.ozon.ru/images/main.jpeg".to_string(),
        image_bytes: cursor.into_inner(),
    }
}

#[test]
fn ozon_cache_root_follows_output_anchor_parent() {
    let output_anchor = PathBuf::from("/tmp/input.xlsx");
    assert_eq!(
        cache_root_for_output_anchor(&output_anchor),
        PathBuf::from("/tmp/.desktop_app_cache/ozon_source")
    );
}

#[test]
fn ozon_source_cache_returns_miss_then_hit_after_store() {
    let root = make_temp_dir("ozon-cache-hit");
    let cache = OzonSourceCache::new(root.clone());
    let product_url = "https://www.ozon.ru/product/3570411009/";

    assert!(matches!(
        cache
            .lookup(product_url)
            .expect("initial lookup should succeed"),
        OzonSourceCacheLookup::Miss
    ));

    let resolution = sample_resolution();
    cache
        .store(product_url, &resolution)
        .expect("store should succeed");

    match cache
        .lookup("https://www.ozon.ru/product/3570411009")
        .expect("lookup should succeed")
    {
        OzonSourceCacheLookup::Hit(cached) => {
            assert_eq!(cached, resolution);
        }
        other => panic!("expected cache hit, got {other:?}"),
    }

    std::fs::remove_dir_all(root).expect("cleanup cache dir");
}

#[test]
fn ozon_source_cache_reports_corrupted_entries() {
    let root = make_temp_dir("ozon-cache-corrupted");
    let cache = OzonSourceCache::new(root.clone());
    let product_url = "https://www.ozon.ru/product/3570411009";
    let resolution = sample_resolution();

    cache
        .store(product_url, &resolution)
        .expect("store should succeed");

    let entry_dir = std::fs::read_dir(&root)
        .expect("cache root should be readable")
        .next()
        .expect("cache entry should exist")
        .expect("cache entry dir should be readable")
        .path();
    std::fs::write(entry_dir.join("meta.json"), b"{invalid-json")
        .expect("corrupt metadata should be writable");

    match cache.lookup(product_url).expect("lookup should succeed") {
        OzonSourceCacheLookup::Corrupted(error) => {
            assert!(
                error.contains("parse ozon cache metadata failed"),
                "unexpected corruption error: {error}"
            );
        }
        other => panic!("expected corrupted cache entry, got {other:?}"),
    }

    std::fs::remove_dir_all(root).expect("cleanup cache dir");
}
