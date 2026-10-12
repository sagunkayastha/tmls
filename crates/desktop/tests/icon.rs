//! The app icon: the committed PNGs are square at their named size, the desktop entry ties the
//! window (`app_id` / WM class `tmls-rs`) to the icon, and the embedded icon decodes.

use std::path::PathBuf;

const SIZES: [u32; 7] = [16, 32, 48, 64, 128, 256, 512];

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn every_committed_icon_is_square_at_its_size() {
    for n in SIZES {
        let path = crate_dir().join(format!("assets/icons/tmls-rs-{n}.png"));
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(
            bytes.len() <= 150 * 1024,
            "{} is {} bytes",
            path.display(),
            bytes.len()
        );
        let img =
            image::load_from_memory(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_eq!((img.width(), img.height()), (n, n), "{}", path.display());
    }
}

#[test]
fn the_desktop_entry_names_the_icon_and_the_window_class() {
    let path = crate_dir().join("../../tooling/desktop/tmls-rs.desktop");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    assert!(lines.contains(&"[Desktop Entry]"), "{text}");
    assert!(lines.contains(&"Icon=tmls-rs"), "{text}");
    assert!(lines.contains(&"StartupWMClass=tmls-rs"), "{text}");
    assert!(lines.contains(&"Type=Application"), "{text}");
}

#[test]
fn the_embedded_window_icon_decodes() {
    assert!(desktop::icon::window_icon().is_some());
    assert_eq!(desktop::icon::APP_ID, "tmls-rs");
}
