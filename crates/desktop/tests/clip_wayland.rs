//! The wl-clipboard fallback for real: `Clipboard` with arboard switched off
//! (`TMLS_RS_NO_ARBOARD=1`) copies through `wl-copy` and pastes through `wl-paste`, each well
//! within a frame or two. Needs a Wayland compositor and wl-clipboard, so it does something only
//! when `TMLS_RS_NO_ARBOARD` is set: tooling/smoke/e2e.sh runs it so in the smoke container.
//! Anywhere else it passes without touching a clipboard.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use desktop::clip::Clipboard;

/// What a copy or a paste through the fallback may take: the UI thread waits for it.
const QUICK: Duration = Duration::from_millis(300);

#[test]
fn the_fallback_copies_and_pastes_at_once() {
    if std::env::var_os("TMLS_RS_NO_ARBOARD").is_none() {
        return;
    }
    assert!(
        std::env::var_os("WAYLAND_DISPLAY").is_some(),
        "no WAYLAND_DISPLAY"
    );
    let mut clip = Clipboard::new();
    let text = format!("wl-fallback-{}", std::process::id());

    let started = Instant::now();
    assert_eq!(clip.set(&text), Ok(()));
    let took = started.elapsed();
    assert!(took < QUICK, "the copy took {took:?}");
    let seen = Command::new("wl-paste")
        .arg("--no-newline")
        .output()
        .expect("wl-paste runs");
    assert_eq!(
        String::from_utf8_lossy(&seen.stdout),
        text,
        "what wl-paste sees"
    );

    // Its daemon would hold this test's stderr, and whoever reads it would wait.
    Command::new("wl-copy")
        .arg("from-outside")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("wl-copy runs");
    let started = Instant::now();
    assert_eq!(clip.get(), Ok("from-outside".to_string()));
    let took = started.elapsed();
    assert!(took < QUICK, "the paste took {took:?}");
    println!("the wl-clipboard fallback: copy and paste each under {QUICK:?}");
}
