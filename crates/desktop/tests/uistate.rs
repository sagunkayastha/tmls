//! The widths the user dragged, as kept in `~/.config/tmls-rs/ui`.

use std::path::{Path, PathBuf};

use desktop::uistate::{self, UiState};

fn state(sidebar_cols: Option<usize>, panel_cols: Option<usize>) -> UiState {
    UiState {
        sidebar_cols,
        panel_cols,
    }
}

/// A directory of this test binary's own, under cargo's target directory.
fn scratch(name: &str) -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("uistate")
        .join(name)
}

#[test]
fn what_is_rendered_parses_back() {
    for x in [
        state(None, None),
        state(Some(34), None),
        state(None, Some(61)),
        state(Some(20), Some(40)),
        state(Some(usize::MAX), Some(1)),
    ] {
        assert_eq!(uistate::parse(&uistate::render(&x)), x, "{x:?}");
    }
    assert_eq!(
        uistate::render(&state(Some(34), Some(61))),
        "sidebar_cols=34\npanel_cols=61\n"
    );
    assert_eq!(uistate::render(&state(None, Some(61))), "panel_cols=61\n");
    assert_eq!(uistate::render(&UiState::default()), "");
}

#[test]
fn lines_that_say_nothing_are_ignored() {
    assert_eq!(UiState::default(), state(None, None));
    for garbage in [
        "",
        "\n\n",
        "garbage",
        "panel_cols",
        "panel_cols=",
        "panel_cols=wide",
        "panel_cols=-3",
        "panel_cols=4.5",
        "panel_cols=99999999999999999999999999",
        "PANEL_COLS=40",
        "panel_rows=40\nsidebar=3\n=7\n\u{0}\u{1b}[31m",
        "# panel_cols=40",
    ] {
        assert_eq!(uistate::parse(garbage), UiState::default(), "{garbage:?}");
    }
    // What can be read is, whatever stands around it; spaces and `\r` are not part of it.
    assert_eq!(
        uistate::parse("future_key=1\n panel_cols = 40 \r\nrubbish\nsidebar_cols=33\n"),
        state(Some(33), Some(40))
    );
    // The last line of a key is the one that counts; one that cannot be read changes nothing.
    assert_eq!(
        uistate::parse("panel_cols=40\npanel_cols=50\npanel_cols=x\n"),
        state(None, Some(50))
    );
    // A width of no columns is none.
    assert_eq!(
        uistate::parse("panel_cols=0\nsidebar_cols=0\n"),
        UiState::default()
    );
}

#[test]
fn the_file_is_ui_in_tmls_rs_under_the_config_directory() {
    let path = |xdg, home| uistate::path(xdg, home);
    assert_eq!(
        path(None, Some("/h")),
        Some(PathBuf::from("/h/.config/tmls-rs/ui"))
    );
    assert_eq!(
        path(Some("/x/cfg"), Some("/h")),
        Some(PathBuf::from("/x/cfg/tmls-rs/ui")),
        "XDG_CONFIG_HOME wins"
    );
    assert_eq!(
        path(Some("/x/cfg"), None),
        Some(PathBuf::from("/x/cfg/tmls-rs/ui"))
    );
    // Empty or relative, as the XDG spec says: not used.
    assert_eq!(
        path(Some(""), Some("/h")),
        Some(PathBuf::from("/h/.config/tmls-rs/ui"))
    );
    assert_eq!(
        path(Some("rel/cfg"), Some("/h")),
        Some(PathBuf::from("/h/.config/tmls-rs/ui"))
    );
    assert_eq!(path(None, None), None);
    assert_eq!(path(None, Some("")), None);
    assert_eq!(path(Some("rel"), Some("rel")), None);
    // Never tmls's own directory.
    let ours = path(None, Some("/h")).unwrap();
    assert!(!ours.starts_with("/h/.config/tmls"));
}

#[test]
fn a_missing_or_unreadable_file_is_the_default() {
    let dir = scratch("missing");
    assert_eq!(
        uistate::load(&dir.join("never").join("ui")),
        UiState::default()
    );
    // A directory where the file should be.
    std::fs::create_dir_all(dir.join("is-a-dir")).unwrap();
    assert_eq!(uistate::load(&dir.join("is-a-dir")), UiState::default());
    // Bytes that are no text.
    std::fs::write(dir.join("binary"), [0xff, 0xfe, b'\n', 0x00]).unwrap();
    assert_eq!(uistate::load(&dir.join("binary")), UiState::default());
}

#[test]
fn saving_makes_the_directory_and_loading_gives_the_same_back() {
    let file = scratch("saved").join("deeper").join("tmls-rs").join("ui");
    let first = state(Some(31), Some(44));
    uistate::save(&file, &first).unwrap();
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "sidebar_cols=31\npanel_cols=44\n"
    );
    assert_eq!(uistate::load(&file), first);
    // Saved again: the file is replaced, not added to.
    let second = state(None, Some(52));
    uistate::save(&file, &second).unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "panel_cols=52\n");
    assert_eq!(uistate::load(&file), second);
}

#[test]
fn a_file_that_cannot_be_written_is_an_error_not_a_panic() {
    // The parent is a file: no directory can be made there.
    let dir = scratch("blocked");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("file"), "x").unwrap();
    let error = uistate::save(&dir.join("file").join("ui"), &state(None, Some(40)));
    assert!(error.is_err());
}
