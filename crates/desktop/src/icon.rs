//! The window's identity: the app id the desktop maps to `tmls-rs.desktop` (Wayland takes the
//! icon from that entry) and the embedded icon X11 shows.

use winit::window::Icon;

/// The Wayland `app_id` and X11 WM class; `StartupWMClass=` in `tooling/desktop/tmls-rs.desktop`.
pub const APP_ID: &str = "tmls-rs";

const PNG: &[u8] = include_bytes!("../assets/icons/tmls-rs-64.png");

/// The 64 px icon for the window. A PNG that does not decode is logged and gives no icon, never
/// a panic.
pub fn window_icon() -> Option<Icon> {
    let decoded = image::load_from_memory(PNG)
        .map_err(|e| e.to_string())
        .and_then(|img| {
            let rgba = img.into_rgba8();
            let (w, h) = rgba.dimensions();
            Icon::from_rgba(rgba.into_raw(), w, h).map_err(|e| e.to_string())
        });
    match decoded {
        Ok(icon) => Some(icon),
        Err(e) => {
            crate::errln(format_args!("tmls-rs: window icon: {e}"));
            None
        }
    }
}
