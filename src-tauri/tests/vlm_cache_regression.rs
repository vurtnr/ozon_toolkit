#[path = "../src/core/matcher.rs"]
mod matcher;
#[path = "../src/core/search_image.rs"]
mod search_image;
#[path = "../src/core/types.rs"]
mod types;
#[path = "../src/core/vlm.rs"]
mod vlm;

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use image::{DynamicImage, Rgba, RgbaImage};
use types::Candidate;
use vlm::create_grid_artifact_with_cache_loader;

fn candidate(url: &str, title: &str) -> Candidate {
    Candidate {
        title: title.to_string(),
        price: "¥1.00".to_string(),
        sales: "".to_string(),
        item_url: format!("https://detail.1688.com/offer/{title}.html"),
        image_url: url.to_string(),
        cos_score_permille: 0,
    }
}

#[test]
fn repeated_candidate_urls_reuse_cached_tiles() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let calls = Arc::clone(&call_count);
    let candidates = vec![
        candidate("https://img.1688.com/shared.jpg", "A"),
        candidate("https://img.1688.com/shared.jpg", "B"),
        candidate("https://img.1688.com/unique.jpg", "C"),
    ];

    let mut cache = std::collections::HashMap::new();
    let artifact = create_grid_artifact_with_cache_loader(&candidates, &mut cache, |url, size| {
        calls.fetch_add(1, Ordering::SeqCst);
        let color = if url.contains("shared") {
            Rgba([255, 0, 0, 255])
        } else {
            Rgba([0, 255, 0, 255])
        };
        Some(DynamicImage::ImageRgba8(RgbaImage::from_pixel(
            size, size, color,
        )))
    });
    let second_artifact =
        create_grid_artifact_with_cache_loader(&candidates, &mut cache, |_, _| {
            panic!("cached urls should not be loaded again on the same task")
        });

    assert!(artifact.is_some(), "grid artifact should still be produced");
    assert!(
        second_artifact.is_some(),
        "subsequent grid builds should reuse cached tiles",
    );
    assert_eq!(
        call_count.load(Ordering::SeqCst),
        2,
        "identical candidate image urls should only be loaded once per task",
    );
}
