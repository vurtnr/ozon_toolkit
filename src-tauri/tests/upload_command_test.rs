use std::path::PathBuf;

use desktop_app_lib::commands::upload::copy_file_in_chunks;

fn temp_path(prefix: &str, ext: &str) -> PathBuf {
    let file_name = format!(
        "{}-{}-{}.{}",
        prefix,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time should be monotonic")
            .as_nanos(),
        ext
    );
    std::env::temp_dir().join(file_name)
}

#[test]
fn copy_file_in_chunks_reports_progress_and_copies_content() {
    let source = temp_path("upload-source", "xlsx");
    let target = temp_path("upload-target", "xlsx");

    // 5 MiB payload to ensure multi-chunk progress.
    let content = vec![0x5a_u8; 5 * 1024 * 1024];
    std::fs::write(&source, &content).expect("write source");

    let mut progress_points = Vec::<(u64, u64)>::new();
    copy_file_in_chunks(&source, &target, 1024 * 1024, |uploaded, total| {
        progress_points.push((uploaded, total));
        Ok(())
    })
    .expect("chunk copy should succeed");

    let target_content = std::fs::read(&target).expect("read target");
    assert_eq!(target_content, content);

    assert!(progress_points.len() >= 5, "expected progress per chunk");
    let (last_uploaded, last_total) = progress_points
        .last()
        .copied()
        .expect("at least one progress callback");
    assert_eq!(last_uploaded, last_total);
    assert_eq!(last_total, content.len() as u64);

    let _ = std::fs::remove_file(source);
    let _ = std::fs::remove_file(target);
}
