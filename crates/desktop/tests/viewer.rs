//! A clicked file: what goes to the panel and what to the desktop's app, the commands that
//! fetch it, the copy of another host's file, and the panel's own state. No command runs
//! here: the runner is a stand-in that answers as a host would.

use std::cell::RefCell;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use client_core::SessionKey;
use client_core::files::{locate_argv, read_argv};
use desktop::links::FileLink;
use desktop::viewer::{
    self, COPY_TIMEOUT, Document, LOCATE_TIMEOUT, MAX_BYTES, MAX_OUTSIDE_BYTES, Opened,
    READ_TIMEOUT, SideBody, SidePanel, TextBody, expand, extension, opens_outside,
};

/// This machine's name; `nas` is another host.
const LOCAL: &str = "laptop";

fn key(host: &str) -> SessionKey {
    SessionKey {
        host: host.to_string(),
        name: "work".to_string(),
    }
}

fn link(path: &str, line: Option<usize>) -> FileLink {
    FileLink {
        path: path.to_string(),
        line,
    }
}

/// A host that has one file: what its locate command prints (or says is wrong) and the
/// file's bytes. Remembers every command it was asked to run.
struct Host {
    located: Result<String, String>,
    content: Vec<u8>,
    calls: RefCell<Vec<(Vec<String>, Duration)>>,
}

impl Host {
    fn new(located: &str, content: &[u8]) -> Host {
        Host {
            located: Ok(located.to_string()),
            content: content.to_vec(),
            calls: RefCell::new(Vec::new()),
        }
    }

    fn failing(error: &str) -> Host {
        Host {
            located: Err(error.to_string()),
            content: Vec::new(),
            calls: RefCell::new(Vec::new()),
        }
    }

    /// As `head -c N` would: the read commands are the ones that start with it.
    fn run(&self, argv: &[String], timeout: Duration) -> anyhow::Result<Vec<u8>> {
        self.calls.borrow_mut().push((argv.to_vec(), timeout));
        let command = argv.last().unwrap();
        let cap = match argv[0].as_str() {
            "head" => Some(argv[2].as_str()),
            _ => command
                .strip_prefix("head -c ")
                .map(|rest| rest.split(' ').next().unwrap()),
        };
        match cap {
            Some(cap) => {
                let cap: usize = cap.parse().unwrap();
                Ok(self.content[..self.content.len().min(cap)].to_vec())
            }
            None => match &self.located {
                Ok(out) => Ok(out.clone().into_bytes()),
                Err(error) => Err(anyhow::anyhow!("{error}")),
            },
        }
    }

    fn open(&self, host: &str, link: &FileLink, cache: Option<&Path>) -> Result<Opened, String> {
        viewer::open_link(&key(host), link, LOCAL, cache, &|argv, timeout| {
            self.run(argv, timeout)
        })
    }

    fn calls(&self) -> Vec<(Vec<String>, Duration)> {
        self.calls.borrow().clone()
    }
}

/// A fresh directory of this test's own, under cargo's target tmp dir.
fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("viewer-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the test's directory");
    dir
}

fn target(opened: Result<Opened, String>) -> String {
    match opened {
        Ok(Opened::Outside { target, .. }) => target,
        other => panic!("not for the desktop's app: {other:?}"),
    }
}

#[test]
fn the_extension_decides_as_in_tmls() {
    assert_eq!(extension("/a/b/shot.PNG"), "png");
    assert_eq!(extension("x.tar.gz"), "gz");
    assert_eq!(extension("Makefile"), "");
    assert_eq!(extension("/a.d/file"), "", "the dot is the folder's");
    assert_eq!(extension(".png"), "", "a hidden file called png");
    assert_eq!(extension("..png"), "");
    assert_eq!(extension(".config.toml"), "toml");
    assert_eq!(extension("a."), "");
    for outside in [
        "a.png", "A.JPEG", "doc.pdf", "v.mkv", "s.flac", "x.tar.gz", "b.tgz", "r.docx", "p.odp",
        "i.svg",
    ] {
        assert!(opens_outside(outside, false), "{outside}");
    }
    for inside in [
        "main.rs",
        "notes.md",
        "Makefile",
        ".png",
        "data.json",
        "a.pngx",
        "log.txt.1",
    ] {
        assert!(!opens_outside(inside, false), "{inside}");
    }
    assert!(opens_outside("src", true), "a folder");
    assert_eq!(viewer::OUTSIDE.len(), 35);
    assert_eq!((MAX_BYTES, MAX_OUTSIDE_BYTES), (2_000_000, 25_000_000));
}

#[test]
fn a_text_file_of_this_machine_is_located_then_read() {
    let host = Host::new("f\n13\n/w/src/main.rs", b"fn main() {}\n");
    let opened = host.open(LOCAL, &link("src/main.rs", Some(3)), None);
    assert_eq!(
        opened,
        Ok(Opened::Text(Document {
            host: LOCAL.to_string(),
            path: "/w/src/main.rs".to_string(),
            line: Some(3),
            text: "fn main() {}\n".to_string(),
        }))
    );
    // The relative path goes to the session's pane; what is read is the path that gave,
    // one byte more than the panel takes, each with its time limit.
    assert_eq!(
        host.calls(),
        vec![
            (
                locate_argv(&key(LOCAL), "src/main.rs", LOCAL),
                LOCATE_TIMEOUT
            ),
            (
                read_argv(LOCAL, "/w/src/main.rs", 2_000_001, LOCAL),
                READ_TIMEOUT
            ),
        ]
    );
    assert_eq!(host.calls()[0].0[0], "sh");
    assert_eq!(host.calls()[1].0[0], "head");
}

#[test]
fn a_text_file_of_another_host_comes_through_ssh() {
    let host = Host::new("f\n6\n/home/u/a b.txt", b"hello\n");
    let opened = host.open("nas", &link("~/a b.txt", None), None);
    let Ok(Opened::Text(document)) = opened else {
        panic!("not text: {opened:?}");
    };
    assert_eq!((document.host.as_str(), document.line), ("nas", None));
    assert_eq!(document.path, "/home/u/a b.txt");
    let calls = host.calls();
    assert_eq!(calls.len(), 2);
    for (argv, _) in &calls {
        assert_eq!(&argv[..2], ["ssh", "-o"], "{argv:?}");
        assert_eq!(argv.len(), 7, "the command is one argument: {argv:?}");
    }
    assert!(calls[0].0[6].starts_with("p=\"$HOME\"/'a b.txt'; "));
    assert_eq!(calls[1].0[6], "head -c 2000001 -- '/home/u/a b.txt'");
}

#[test]
fn what_is_not_text_goes_to_the_desktops_app_where_it_is_on_this_machine() {
    // By its name: not read at all.
    let host = Host::new("f\n500\n/w/shot.PNG", b"\x89PNG");
    assert_eq!(
        target(host.open(LOCAL, &link("shot.PNG", None), None)),
        "/w/shot.PNG"
    );
    assert_eq!(host.calls().len(), 1, "located, not read");
    // Also when a line was given (tmls: an image never fits the text viewer).
    let host = Host::new("f\n500\n/w/doc.pdf", b"%PDF");
    assert_eq!(
        target(host.open(LOCAL, &link("doc.pdf", Some(3)), None)),
        "/w/doc.pdf"
    );
    // A folder.
    let host = Host::new("d\n0\n/w/src", b"");
    assert_eq!(target(host.open(LOCAL, &link("src", None), None)), "/w/src");
    assert_eq!(host.calls().len(), 1);
    // Too large for the panel: by its size, without reading it.
    let host = Host::new("f\n2000001\n/w/big.log", b"");
    assert_eq!(
        target(host.open(LOCAL, &link("big.log", None), None)),
        "/w/big.log"
    );
    assert_eq!(host.calls().len(), 1);
    // Not UTF-8, and text with a NUL byte: read first, then the desktop's.
    for bytes in [&b"caf\xe9\n"[..], &b"a\0b\n"[..], &[0xff, 0xfe, 0x00][..]] {
        let host = Host::new("f\n5\n/w/data.txt", bytes);
        assert_eq!(
            target(host.open(LOCAL, &link("data.txt", None), None)),
            "/w/data.txt"
        );
        assert_eq!(host.calls().len(), 2, "located and read");
    }
    // It grew past the panel's size after it was located.
    let host = Host::new("f\n10\n/w/grows.log", &vec![b'a'; MAX_BYTES + 5]);
    assert_eq!(
        target(host.open(LOCAL, &link("grows.log", None), None)),
        "/w/grows.log"
    );
}

#[test]
fn the_panel_takes_exactly_two_million_bytes() {
    let host = Host::new("f\n2000000\n/w/full.log", &vec![b'a'; MAX_BYTES]);
    let opened = host.open(LOCAL, &link("full.log", None), None);
    let Ok(Opened::Text(document)) = opened else {
        panic!("not text");
    };
    assert_eq!(document.text.len(), MAX_BYTES);
}

#[test]
fn a_file_of_another_host_is_copied_to_the_cache_and_the_copy_opened() {
    let cache = scratch("copy");
    let host = Host::new("f\n4\n/srv/pics/it's a shot.png", b"\x89PNG");
    let opened = host.open(
        "nas",
        &link("/srv/pics/it's a shot.png", None),
        Some(&cache),
    );
    let copy = cache.join("nas").join("it's a shot.png");
    assert_eq!(target(opened), copy.to_str().unwrap());
    assert_eq!(std::fs::read(&copy).unwrap(), b"\x89PNG");
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(&copy).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600, "the user's only, and not to be run");
    let calls = host.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(
        calls[1],
        (
            read_argv("nas", "/srv/pics/it's a shot.png", 25_000_001, LOCAL),
            COPY_TIMEOUT
        )
    );
    // The next file of that name from that host replaces the copy.
    let host = Host::new("f\n3\n/other/it's a shot.png", b"new");
    host.open("nas", &link("/other/it's a shot.png", None), Some(&cache))
        .unwrap();
    assert_eq!(std::fs::read(&copy).unwrap(), b"new");

    // Text that turned out to be none: the bytes read for the panel are the copy.
    let host = Host::new("f\n4\n/srv/data.txt", b"\xff\xfe\x00\x01");
    let opened = host.open("nas", &link("/srv/data.txt", Some(2)), Some(&cache));
    assert_eq!(target(opened), cache.join("nas/data.txt").to_str().unwrap());
    assert_eq!(
        std::fs::read(cache.join("nas/data.txt")).unwrap(),
        b"\xff\xfe\x00\x01"
    );
    assert_eq!(host.calls().len(), 2, "not fetched a second time");
    let _ = std::fs::remove_dir_all(&cache);
}

#[test]
fn another_hosts_folder_and_too_large_a_file_are_refused() {
    let cache = scratch("refused");
    let host = Host::new("d\n0\n/srv/pics", b"");
    let said = host
        .open("nas", &link("/srv/pics", None), Some(&cache))
        .unwrap_err();
    assert_eq!(
        said,
        "/srv/pics on nas: a folder of another host is not opened"
    );
    // One byte over the limit: refused by its size, nothing is fetched.
    let host = Host::new("f\n25000001\n/srv/big.zip", b"");
    let said = host
        .open("nas", &link("/srv/big.zip", None), Some(&cache))
        .unwrap_err();
    assert_eq!(
        said,
        "/srv/big.zip on nas: too large to copy (25.0 MB, the limit is 25 MB)"
    );
    assert_eq!(host.calls().len(), 1);
    // At the limit it is fetched; a file that grew past it meanwhile is not kept.
    let host = Host::new("f\n25000000\n/srv/grew.zip", &vec![7; 25_000_001]);
    let said = host
        .open("nas", &link("/srv/grew.zip", None), Some(&cache))
        .unwrap_err();
    assert!(said.contains("too large to copy"), "{said}");
    assert!(!cache.join("nas/grew.zip").exists());
    let host = Host::new("f\n25000000\n/srv/fits.zip", &vec![7; 25_000_000]);
    let copy = target(host.open("nas", &link("/srv/fits.zip", None), Some(&cache)));
    assert_eq!(std::fs::metadata(copy).unwrap().len(), 25_000_000);
    // Text too large for the panel is copied like the rest.
    let host = Host::new("f\n2000001\n/srv/big.log", &vec![b'a'; 2_000_001]);
    let copy = target(host.open("nas", &link("/srv/big.log", Some(9)), Some(&cache)));
    assert_eq!(std::fs::metadata(copy).unwrap().len(), 2_000_001);
    // Nowhere to copy to.
    let host = Host::new("f\n4\n/srv/a.png", b"\x89PNG");
    let said = host
        .open("nas", &link("/srv/a.png", None), None)
        .unwrap_err();
    assert_eq!(said, "/srv/a.png on nas: no cache directory to copy it to");
    assert_eq!(host.calls().len(), 1);
    let _ = std::fs::remove_dir_all(&cache);
}

#[test]
fn what_goes_wrong_is_one_line_that_names_the_file() {
    let host = Host::failing("not found");
    assert_eq!(
        host.open(LOCAL, &link("src/x.rs", Some(1)), None),
        Err("src/x.rs: not found".to_string())
    );
    assert_eq!(
        host.open("nas", &link("src/x.rs", Some(1)), None),
        Err("src/x.rs on nas: not found".to_string())
    );
    let host = Host::failing("\n  ssh: connect to host nas port 22: Connection timed out\nmore\n");
    assert_eq!(
        host.open("nas", &link("/x", None), None),
        Err("/x on nas: ssh: connect to host nas port 22: Connection timed out".to_string())
    );
    // An answer that is none, and a path the host did not make absolute.
    for garbage in ["", "Welcome to nas!\n", "f\n1\nrelative/path"] {
        let host = Host::new(garbage, b"x");
        let said = host.open("nas", &link("a.txt", None), None).unwrap_err();
        assert_eq!(said, "a.txt on nas: the host did not say where that is");
        assert_eq!(host.calls().len(), 1, "nothing is read after that");
    }
    // A device or a pipe is never read.
    let host = Host::new("o\n0\n/dev/zero", b"");
    assert_eq!(
        host.open(LOCAL, &link("/dev/zero", None), None),
        Err("/dev/zero: not a file or a folder".to_string())
    );
    assert_eq!(host.calls().len(), 1);
}

#[test]
fn open_in_app_takes_the_panels_file_as_it_is() {
    let cache = scratch("in-app");
    let run = |host: &Host, on: &str| {
        viewer::open_in_app(on, "/w/notes.md", LOCAL, Some(&cache), &|argv, timeout| {
            host.run(argv, timeout)
        })
    };
    // Here: the file itself, text or not.
    let host = Host::new("f\n6\n/w/notes.md", b"hello\n");
    assert_eq!(target(run(&host, LOCAL)), "/w/notes.md");
    assert_eq!(host.calls().len(), 1);
    assert!(
        host.calls()[0].0[2].starts_with("p='/w/notes.md'; "),
        "no session is asked"
    );
    // Elsewhere: a copy.
    let host = Host::new("f\n6\n/w/notes.md", b"hello\n");
    let copy = target(run(&host, "nas"));
    assert_eq!(std::fs::read(copy).unwrap(), b"hello\n");
    // Gone since it was opened.
    let host = Host::failing("not found");
    assert_eq!(
        run(&host, "nas"),
        Err("/w/notes.md on nas: not found".to_string())
    );
    let _ = std::fs::remove_dir_all(&cache);
}

#[test]
fn the_cache_is_under_xdg_cache_home_or_the_home() {
    let os = |text: &'static str| Some(OsStr::new(text));
    assert_eq!(
        viewer::cache_dir(os("/x/cache"), os("/home/u")),
        Some(PathBuf::from("/x/cache/tmls-rs/view"))
    );
    assert_eq!(
        viewer::cache_dir(None, os("/home/u")),
        Some(PathBuf::from("/home/u/.cache/tmls-rs/view"))
    );
    // Empty or relative does not count.
    assert_eq!(
        viewer::cache_dir(os(""), os("/home/u")),
        Some(PathBuf::from("/home/u/.cache/tmls-rs/view"))
    );
    assert_eq!(
        viewer::cache_dir(os("cache"), os("/home/u")),
        Some(PathBuf::from("/home/u/.cache/tmls-rs/view"))
    );
    assert_eq!(viewer::cache_dir(None, None), None);
    assert_eq!(viewer::cache_dir(os("rel"), os("")), None);

    let cache = Path::new("/c");
    let at = |host: &str, path: &str| viewer::cache_path(cache, host, path);
    assert_eq!(at("nas", "/a/b/x.png"), Some(PathBuf::from("/c/nas/x.png")));
    assert_eq!(
        at("u@nas.lan", "/x.png"),
        Some(PathBuf::from("/c/u@nas.lan/x.png"))
    );
    assert_eq!(at("nas", "/a/b/"), Some(PathBuf::from("/c/nas/b")));
    // A host's name never leaves the cache, a file's is never a directory.
    assert_eq!(
        at("../../etc", "/x"),
        Some(PathBuf::from("/c/_.._.._etc/x"))
    );
    assert_eq!(at("a/b", "/x"), Some(PathBuf::from("/c/a_b/x")));
    assert_eq!(at("", "/x"), Some(PathBuf::from("/c/_/x")));
    assert_eq!(at("..", "/x"), Some(PathBuf::from("/c/_../x")));
    for nameless in ["/", "", "/a/..", "/a/.", "//"] {
        assert_eq!(at("nas", nameless), None, "{nameless:?}");
    }
}

fn numbered(lines: usize) -> String {
    (1..=lines).map(|n| format!("line {n}\n")).collect()
}

fn numbers(body: &TextBody, visible: usize) -> Vec<usize> {
    body.rows(visible, 40)
        .iter()
        .map(|row| row.number)
        .collect()
}

#[test]
fn lines_are_split_as_an_editor_shows_them() {
    let count = |text: &str| TextBody::new(text, None).line_count();
    assert_eq!(count("a\nb\n"), 2);
    assert_eq!(count("a\nb"), 2);
    assert_eq!(count("a\n\nb\n\n"), 4, "empty lines are lines");
    assert_eq!(count(""), 1);
    assert_eq!(count("\n"), 1);
    let body = TextBody::new("\u{feff}one\r\ntwo\r\n", None);
    let rows = body.rows(10, 40);
    assert_eq!(rows.len(), 2);
    assert_eq!(
        (rows[0].text.as_str(), rows[1].text.as_str()),
        ("one", "two")
    );
    assert_eq!((rows[0].number, rows[1].number), (1, 2));
    assert!(rows.iter().all(|row| !row.marked), "no line was asked for");
}

#[test]
fn the_gutter_is_as_wide_as_the_last_line_number_and_a_space() {
    for (lines, gutter) in [(1, 2), (9, 2), (10, 3), (99, 3), (100, 4), (12345, 6)] {
        let body = TextBody::new(&numbered(lines), None);
        assert_eq!(body.line_count(), lines);
        assert_eq!(body.gutter_cols(), gutter, "{lines} lines");
    }
}

#[test]
fn it_opens_with_the_asked_line_a_third_down_and_marked() {
    let mut body = TextBody::new(&numbered(100), Some(50));
    body.reveal(20);
    assert_eq!(numbers(&body, 20), (44..=63).collect::<Vec<_>>());
    let rows = body.rows(20, 40);
    let marked: Vec<usize> = rows
        .iter()
        .filter(|row| row.marked)
        .map(|row| row.number)
        .collect();
    assert_eq!(marked, [50]);
    assert_eq!(rows[6].text, "line 50");

    // Near the top there is nothing to scroll; near the end the file's end is at the foot.
    let mut body = TextBody::new(&numbered(100), Some(3));
    body.reveal(20);
    assert_eq!(numbers(&body, 20)[0], 1);
    let mut body = TextBody::new(&numbered(100), Some(100));
    body.reveal(20);
    assert_eq!(numbers(&body, 20), (81..=100).collect::<Vec<_>>());
    assert!(body.rows(20, 40)[19].marked);
    // A line past the end marks the last one; line 0 the first (tmls clamps).
    let mut body = TextBody::new(&numbered(100), Some(5000));
    body.reveal(20);
    assert!(body.rows(20, 40)[19].marked);
    let body = TextBody::new(&numbered(100), Some(0));
    assert!(body.rows(20, 40)[0].marked);
    // Without a line: the top, nothing marked.
    let mut body = TextBody::new(&numbered(100), None);
    body.reveal(20);
    assert_eq!(numbers(&body, 20)[0], 1);
}

#[test]
fn scrolling_stops_at_both_ends() {
    let mut body = TextBody::new(&numbered(100), Some(50));
    body.reveal(20);
    assert_eq!(body.max_top(20), 80);
    assert!(body.scroll(-5, 20));
    assert_eq!(numbers(&body, 20)[0], 39);
    assert!(body.scroll(-1000, 20));
    assert_eq!(numbers(&body, 20)[0], 1);
    assert!(!body.scroll(-3, 20), "already at the top");
    assert!(body.scroll(i32::MAX, 20));
    assert_eq!(numbers(&body, 20), (81..=100).collect::<Vec<_>>());
    assert!(!body.scroll(3, 20), "already at the end");
    assert!(body.scroll(i32::MIN, 20));
    assert_eq!(numbers(&body, 20)[0], 1);

    // A taller panel after a scroll to the end: still full, the end at its foot.
    body.scroll(1000, 20);
    assert_eq!(numbers(&body, 50), (51..=100).collect::<Vec<_>>());
    // A file shorter than the panel does not scroll.
    let mut short = TextBody::new(&numbered(5), Some(4));
    short.reveal(20);
    assert!(!short.scroll(3, 20) && !short.scroll(-3, 20));
    assert_eq!(numbers(&short, 20), [1, 2, 3, 4, 5]);
    // No room at all: nothing in view, nothing breaks.
    assert_eq!(numbers(&body, 0), Vec::<usize>::new());
    body.reveal(0);
    body.scroll(1, 0);
}

fn width(text: &str) -> usize {
    s2_wgpu::gfx::grid::clusters(text)
        .iter()
        .map(|(_, width)| width)
        .sum()
}

#[test]
fn tabs_go_to_the_next_stop_also_after_wide_characters() {
    assert_eq!(expand("\tx", 80), "    x");
    assert_eq!(expand("ab\tc", 80), "ab  c");
    assert_eq!(expand("abcd\te", 80), "abcd    e");
    assert_eq!(expand("\t\tx", 80), "        x");
    // A CJK character is two columns: the stop is counted in columns, not characters.
    assert_eq!(expand("日\tx", 80), "日  x");
    assert_eq!(expand("日本\tx", 80), "日本    x");
    assert_eq!(expand("日本語\tx", 80), "日本語  x");
    // A mark that takes no column moves nothing.
    assert_eq!(expand("e\u{301}\tx", 80), "e\u{301}   x");
    // So the text after a tab starts on the same column whatever is before it.
    for line in ["a\tz", "日\tz", "é\tz", "abc\tz", "\tz"] {
        let out = expand(line, 80);
        assert_eq!(width(&out[..out.len() - 1]), 4, "{line:?} -> {out:?}");
    }
    assert_eq!(expand("plain", 80), "plain");
    assert_eq!(expand("", 80), "");
}

#[test]
fn a_very_long_line_costs_what_the_panel_can_show() {
    let long = "x".repeat(1_500_000);
    let out = expand(&long, 40);
    assert!(out.len() >= 41 && out.len() <= 400, "{}", out.len());
    let wide = "語".repeat(500_000);
    let out = expand(&wide, 40);
    assert!(width(&out) > 40 && out.chars().count() <= 400);
    // And a row of it is still one row.
    let body = TextBody::new(&format!("{long}\nshort\n"), Some(2));
    let rows = body.rows(10, 40);
    assert_eq!(rows.len(), 2);
    assert!(rows[0].text.len() <= 400);
    assert_eq!(rows[1].text, "short");
}

#[test]
fn the_panel_names_the_host_unless_it_is_this_machine() {
    let document = |host: &str| Document {
        host: host.to_string(),
        path: "/w/src/main.rs".to_string(),
        line: Some(2),
        text: "a\n\tb\nc\n".to_string(),
    };
    let mut here = SidePanel::text(&document(LOCAL), LOCAL);
    here.reveal(10);
    let view = here.view(10, 40);
    assert_eq!(view.title, "/w/src/main.rs");
    let SideBody::Text { gutter, rows } = &view.body;
    assert_eq!(*gutter, 2);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[1].text, "    b");
    assert!(rows[1].marked && !rows[0].marked);
    assert_eq!(
        (here.host.as_str(), here.path.as_str()),
        (LOCAL, "/w/src/main.rs")
    );

    let there = SidePanel::text(&document("nas"), LOCAL);
    assert_eq!(there.view(10, 40).title, "nas:/w/src/main.rs");
    assert_eq!(there.host, "nas");
    // The wheel moves a panel that has more than fits.
    let mut long = SidePanel::text(
        &Document {
            text: numbered(50),
            ..document(LOCAL)
        },
        LOCAL,
    );
    assert!(long.scroll(3, 10));
    let SideBody::Text { rows, .. } = long.view(10, 40).body;
    assert_eq!(rows[0].number, 4);
    assert!(!here.scroll(3, 10));
}
