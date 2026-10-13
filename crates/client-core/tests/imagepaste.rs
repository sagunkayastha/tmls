//! Image paste: the clipboard's image staged on this machine, then copied to the session's
//! host. The clipboard and the copy are stand-ins (no `wl-paste`, no ssh, no host); the files
//! go under the build's temporary directory.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use client_core::imagedrop::store_argv;
use client_core::imagepaste::{NO_IMAGE, NO_WL_PASTE, Pasted, extension, paste, pick_type, stage, stamp_name};
use client_core::tmux;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nnot-really-a-picture";
const NAME: &str = "1791777302-123456.png";

fn now() -> std::time::SystemTime {
    UNIX_EPOCH + Duration::new(1_791_777_302, 123_456_000)
}

/// A staging folder of this test's own, not made yet.
fn staging(test: &str) -> PathBuf {
    let run = std::time::SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let home = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("imagepaste-{test}-{}-{run}", std::process::id()));
    home.join(".cache/tmls-rs/paste")
}

fn staged(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

/// A clipboard that offers `types` and gives `bytes` for the image type asked for.
fn clipboard(types: &'static str, bytes: &'static [u8]) -> impl Fn(&[String]) -> anyhow::Result<Vec<u8>> {
    move |argv: &[String]| {
        let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
        match argv.as_slice() {
            ["wl-paste", "--list-types"] => Ok(types.as_bytes().to_vec()),
            ["wl-paste", "--type", mime] if types.lines().any(|line| line == *mime) => Ok(bytes.to_vec()),
            other => anyhow::bail!("unexpected {other:?}"),
        }
    }
}

fn never_fed(_: &[String], _: &[u8]) -> anyhow::Result<String> {
    panic!("nothing is copied without an image")
}

fn no_note(note: String) {
    panic!("nothing to say: {note}")
}

#[test]
fn the_clipboard_type_and_the_file_name() {
    assert_eq!(pick_type("text/plain\nimage/jpeg\nimage/png\n"), Some("image/png"));
    assert_eq!(pick_type("image/jpeg\r\n"), Some("image/jpeg"));
    assert_eq!(pick_type("text/plain\nimage/webp\nimage/pngx\n"), None);
    assert_eq!(pick_type(""), None);
    assert_eq!((extension("image/png"), extension("image/jpeg")), ("png", "jpg"));
    assert_eq!(stamp_name(now(), "png").as_deref(), Some(NAME));
    assert_eq!(stamp_name(UNIX_EPOCH, "jpg").as_deref(), Some("0-000000.jpg"));
    assert_eq!(stamp_name(now(), "txt"), None);
}

#[test]
fn a_session_here_gets_the_staged_file_itself() {
    let dir = staging("local");
    let got = paste(None, &dir, now(), &clipboard("text/plain\nimage/png\n", PNG), &never_fed, &no_note);
    let file = dir.join(NAME);
    assert_eq!(got, Ok(Pasted { name: NAME.to_string(), path: file.to_string_lossy().into_owned() }));
    assert_eq!(std::fs::read(&file).unwrap(), PNG);
    // The same instant again: the file that is there is not written over.
    let again = stage(&dir, now(), &clipboard("image/png\n", b"other bytes"));
    assert!(again.unwrap_err().starts_with("couldn't write "), "a second file of that name");
    assert_eq!(std::fs::read(&file).unwrap(), PNG);
    // A JPEG when there is no PNG.
    let later = now() + Duration::from_secs(1);
    let jpeg = stage(&dir, later, &clipboard("image/jpeg\n", b"jpeg bytes")).unwrap();
    assert_eq!(jpeg, dir.join("1791777303-123456.jpg"));
    assert_eq!(staged(&dir), ["1791777302-123456.png", "1791777303-123456.jpg"]);
}

#[test]
fn a_session_on_another_host_gets_the_path_of_the_copy_there() {
    let dir = staging("remote");
    let fed = RefCell::new(Vec::new());
    let feed = |argv: &[String], input: &[u8]| {
        fed.borrow_mut().push((argv.to_vec(), input.to_vec()));
        Ok(String::new())
    };
    let notes = RefCell::new(Vec::new());
    let note = |text: String| notes.borrow_mut().push(text);
    let got = paste(Some("ph"), &dir, now(), &clipboard("image/png\n", PNG), &feed, &note);
    let path = format!("~/.cache/tmls/images/{NAME}");
    assert_eq!(got, Ok(Pasted { name: NAME.to_string(), path }));
    assert_eq!(fed.into_inner(), [(store_argv("ph", NAME), PNG.to_vec())]);
    assert_eq!(notes.into_inner(), [format!("copying {NAME} to ph …")]);
    assert_eq!(staged(&dir), [NAME], "the staged file is not removed");
}

#[test]
fn a_failed_copy_types_nothing_and_says_why() {
    let dir = staging("failed");
    // The copy fails: the status line says so, there is no path to type, the staged file stays.
    let tried = RefCell::new(0);
    let refused = |_: &[String], _: &[u8]| -> anyhow::Result<String> {
        *tried.borrow_mut() += 1;
        anyhow::bail!("ssh: connect to host ph port 22: Connection refused\nmore")
    };
    assert_eq!(
        paste(Some("ph"), &dir, now(), &clipboard("image/png\n", PNG), &refused, &|_| ()),
        Err("copy to ph failed: ssh: connect to host ph port 22: Connection refused".to_string())
    );
    assert_eq!(*tried.borrow(), 1);
    assert_eq!(std::fs::read(dir.join(NAME)).unwrap(), PNG);

    // No image: only text, an image of another type, nothing at all, an image of no bytes.
    let none = dir.join("none");
    let no_image = Err(NO_IMAGE.to_string());
    assert_eq!(NO_IMAGE, "clipboard has no image");
    for types in ["text/plain\nUTF8_STRING\n", "image/webp\n", ""] {
        assert_eq!(paste(Some("ph"), &none, now(), &clipboard(types, PNG), &never_fed, &no_note), no_image, "{types:?}");
    }
    let empty = |_: &[String]| -> anyhow::Result<Vec<u8>> { anyhow::bail!("Nothing is copied") };
    assert_eq!(paste(Some("ph"), &none, now(), &empty, &never_fed, &no_note), no_image);
    assert_eq!(paste(None, &none, now(), &clipboard("image/png\n", b""), &never_fed, &no_note), no_image);
    // wl-paste fails some other way: that is not "no image".
    let stuck = |_: &[String]| -> anyhow::Result<Vec<u8>> { anyhow::bail!("wl-paste timed out after 10.0 s") };
    assert_eq!(
        paste(None, &none, now(), &stuck, &never_fed, &no_note),
        Err("couldn't read the clipboard: wl-paste timed out after 10.0 s".to_string())
    );

    // No wl-paste: what `tmux::run_bytes` really says of a program that is not there.
    assert_eq!(NO_WL_PASTE, "wl-paste is not installed: no image paste");
    let missing = |argv: &[String]| {
        assert_eq!(argv[0], "wl-paste");
        let mut argv = argv.to_vec();
        argv[0] = "tmlsrs-test-no-such-wl-paste".to_string();
        tmux::run_bytes(&argv, Duration::from_secs(5))
    };
    assert_eq!(paste(Some("ph"), &none, now(), &missing, &never_fed, &no_note), Err(NO_WL_PASTE.to_string()));
    assert_eq!(staged(&none), Vec::<String>::new(), "nothing is staged without an image");
}
