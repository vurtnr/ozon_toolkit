use std::path::PathBuf;

use desktop_app_lib::core::search_image::{
    generate_search_images, parse_search_image_plan, BackgroundStrategy, NormalizedBBox,
    SearchImagePlan,
};

fn temp_dir_path() -> PathBuf {
    let unique = format!(
        "search-image-pipeline-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time should be monotonic")
            .as_nanos()
    );
    std::env::temp_dir().join(unique)
}

fn write_fixture_image(width: u32, height: u32) -> PathBuf {
    let path = temp_dir_path().join("source.png");
    std::fs::create_dir_all(path.parent().expect("fixture parent")).expect("create fixture dir");

    let img = image::RgbaImage::from_fn(width, height, |x, y| {
        if x > width / 4 && x < (width * 3) / 4 && y > height / 6 && y < (height * 5) / 6 {
            image::Rgba([220, 30, 30, 255])
        } else {
            image::Rgba([40, 90, 160, 255])
        }
    });
    img.save(&path).expect("save fixture image");
    path
}

fn fixture_plan() -> SearchImagePlan {
    SearchImagePlan {
        target_product: "bag".to_string(),
        scene_type: "single_product".to_string(),
        primary_bbox: NormalizedBBox {
            x: 0.18,
            y: 0.12,
            width: 0.56,
            height: 0.68,
        },
        fallback_bbox: NormalizedBBox {
            x: 0.10,
            y: 0.06,
            width: 0.74,
            height: 0.82,
        },
        background_strategy: BackgroundStrategy::RemoveAndWhitefill,
        subject_confidence: 0.92,
        needs_fallback_context: true,
    }
}

#[test]
fn parse_search_image_plan_accepts_strict_json_and_normalizes_boxes() {
    let content = r#"{
      "target_product":"bag",
      "scene_type":"single_product",
      "primary_bbox":{"x":0.18,"y":0.12,"width":0.56,"height":0.68},
      "fallback_bbox":{"x":0.10,"y":0.06,"width":0.74,"height":0.82},
      "background_strategy":"remove_and_whitefill",
      "subject_confidence":0.92,
      "needs_fallback_context":true
    }"#;

    let plan = parse_search_image_plan(content).expect("plan should parse");
    assert_eq!(plan.target_product, "bag");
    assert_eq!(plan.scene_type, "single_product");
    assert!(plan.primary_bbox.width > 0.0);
    assert!(plan.primary_bbox.height > 0.0);
    assert!(plan.primary_bbox.x >= 0.0);
    assert!(plan.primary_bbox.y >= 0.0);
    assert!(plan.fallback_bbox.width >= plan.primary_bbox.width);
    assert!(plan.fallback_bbox.height >= plan.primary_bbox.height);
}

#[test]
fn parse_search_image_plan_rejects_non_json_and_out_of_bounds_boxes() {
    assert!(parse_search_image_plan("not-json").is_err());

    let invalid = r#"{
      "target_product":"bag",
      "scene_type":"single_product",
      "primary_bbox":{"x":1.2,"y":0.12,"width":0.56,"height":0.68},
      "fallback_bbox":{"x":0.10,"y":0.06,"width":0.74,"height":0.82},
      "background_strategy":"remove_and_whitefill",
      "subject_confidence":0.92,
      "needs_fallback_context":true
    }"#;

    assert!(parse_search_image_plan(invalid).is_err());
}

#[test]
fn generate_search_images_writes_square_pngs_for_primary_and_fallback() {
    let source_path = write_fixture_image(400, 300);
    let temp_dir = temp_dir_path();
    std::fs::create_dir_all(&temp_dir).expect("create temp dir");

    let generated = generate_search_images(&source_path, &fixture_plan(), &temp_dir, "row-1")
        .expect("search images should be generated");

    let primary = image::open(&generated.primary_path).expect("open primary");
    let fallback = image::open(&generated.fallback_path).expect("open fallback");

    assert_eq!((primary.width(), primary.height()), (1024, 1024));
    assert_eq!((fallback.width(), fallback.height()), (1024, 1024));
    assert!(generated.primary_path.exists());
    assert!(generated.fallback_path.exists());
}

#[test]
fn invalid_bbox_falls_back_to_normalized_original_image() {
    let source_path = write_fixture_image(400, 300);
    let temp_dir = temp_dir_path();
    std::fs::create_dir_all(&temp_dir).expect("create temp dir");
    let mut invalid_plan = fixture_plan();
    invalid_plan.primary_bbox = NormalizedBBox {
        x: 0.95,
        y: 0.95,
        width: 0.3,
        height: 0.3,
    };

    let generated = generate_search_images(&source_path, &invalid_plan, &temp_dir, "row-2")
        .expect("generation should still succeed");

    assert!(generated.primary_path.exists());
    assert!(generated.fallback_path.exists());
}
