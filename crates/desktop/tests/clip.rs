//! What a copy or a paste says in the status line, and the wl-clipboard fallback's commands. No
//! clipboard and no compositor here: the clipboard is a stand-in.

use std::cell::RefCell;

use desktop::clip::{copy_with, paste_with, wl_copy, wl_paste};

#[test]
fn copy_says_what_happened() {
    assert_eq!(
        copy_with(Some("héllo".into()), |_| Ok(())),
        Ok("copied 5 chars".to_string())
    );
    assert_eq!(
        copy_with(None, |_| panic!("nothing to set")),
        Ok("nothing selected".to_string())
    );
    assert_eq!(
        copy_with(Some(String::new()), |_| panic!("nothing to set")),
        Ok("nothing selected".to_string())
    );
    assert_eq!(
        copy_with(Some("x".into()), |_| Err("no seat".into())),
        Err("copy failed: no seat".to_string())
    );
}

#[test]
fn paste_says_what_happened() {
    let mut put = String::new();
    assert_eq!(
        paste_with(Ok("ab日".into()), |text| put.push_str(text)),
        Ok("pasted 3 chars".to_string())
    );
    assert_eq!(put, "ab日");
    assert_eq!(
        paste_with(Ok(String::new()), |_| panic!("nothing to paste")),
        Ok("clipboard empty".to_string())
    );
    assert_eq!(
        paste_with(Err("no seat".into()), |_| panic!("nothing to paste")),
        Err("paste failed: no seat".to_string())
    );
}

#[test]
fn wl_copy_feeds_the_text_on_stdin() {
    let got = RefCell::new(None);
    let feed = |argv: &[String], input: &[u8]| -> anyhow::Result<String> {
        *got.borrow_mut() = Some((argv.to_vec(), input.to_vec()));
        Ok(String::new())
    };
    assert_eq!(wl_copy("a'b\n", &feed), Ok(()));
    assert_eq!(
        got.into_inner(),
        Some((vec!["wl-copy".to_string()], b"a'b\n".to_vec()))
    );
    let broken = |_: &[String], _: &[u8]| -> anyhow::Result<String> { anyhow::bail!("no wayland") };
    assert_eq!(wl_copy("x", &broken), Err("no wayland".to_string()));
}

#[test]
fn wl_paste_reads_without_a_newline_and_empty_is_no_error() {
    let text = |argv: &[String]| -> anyhow::Result<String> {
        assert_eq!(argv, ["wl-paste", "--no-newline"]);
        Ok("x".into())
    };
    assert_eq!(wl_paste(&text), Ok("x".to_string()));
    let empty = |_: &[String]| -> anyhow::Result<String> { anyhow::bail!("Nothing is copied") };
    assert_eq!(wl_paste(&empty), Ok(String::new()));
    let broken = |_: &[String]| -> anyhow::Result<String> {
        anyhow::bail!("Failed to connect to a Wayland server")
    };
    assert_eq!(
        wl_paste(&broken),
        Err("Failed to connect to a Wayland server".to_string())
    );
}
