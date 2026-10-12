//! `imagedrop::store` against real files (under the build's temporary directory) and a
//! runner that records what it was given: no ssh, no host.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use client_core::imagedrop::{dropped_image, store, store_argv};

fn scratch(test: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("imagedrop-{test}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn store_sends_the_bytes_and_returns_the_path_to_paste() {
    let dir = scratch("store");
    let file = dir.join("pic.png");
    std::fs::write(&file, b"\x89PNG bytes").unwrap();
    let now = UNIX_EPOCH + Duration::new(7, 9_000);

    let seen = RefCell::new(Vec::new());
    let run = |argv: &[String], input: &[u8]| {
        seen.borrow_mut().push((argv.to_vec(), input.to_vec()));
        Ok(String::new())
    };
    assert_eq!(store("nas", &file, now, &run).as_deref(), Ok("~/.cache/tmls/images/7-000009.png"));
    let seen = seen.into_inner();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].0, store_argv("nas", "7-000009.png"));
    assert_eq!(seen[0].1, b"\x89PNG bytes");
}

#[test]
fn store_says_why_it_could_not() {
    let dir = scratch("errors");
    let file = dir.join("pic.jpg");
    std::fs::write(&file, b"jpeg").unwrap();
    let text = dir.join("notes.txt");
    std::fs::write(&text, b"text").unwrap();
    let now = UNIX_EPOCH + Duration::new(7, 9_000);

    let failing = |_: &[String], _: &[u8]| Err(anyhow::anyhow!("Permission denied"));
    assert_eq!(store("nas", &file, now, &failing), Err("Permission denied".to_string()));
    let never = |_: &[String], _: &[u8]| -> anyhow::Result<String> { panic!("nothing to run") };
    assert!(store("nas", &dir.join("missing.png"), now, &never).unwrap_err().contains("missing.png"));
    assert!(store("nas", &text, now, &never).unwrap_err().contains("not an image"));
}

#[test]
fn a_dropped_image_is_one_that_exists() {
    let dir = scratch("exists");
    let file = dir.join("my pic.webp");
    std::fs::write(&file, b"webp").unwrap();
    let path = file.to_string_lossy().into_owned();
    assert_eq!(dropped_image(&path), Some(file.clone()));
    let uri = format!("file://{}", path.replace(' ', "%20"));
    assert_eq!(dropped_image(&uri), Some(file));
    assert_eq!(dropped_image(&dir.join("gone.png").to_string_lossy()), None);
    assert_eq!(dropped_image(&dir.to_string_lossy()), None, "a directory is no image");
}
