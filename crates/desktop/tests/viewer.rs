//! A clicked file: what goes to the panel and what to the desktop's app, the commands that
//! fetch it, the copy of another host's file, and the panel's own state. No command runs
//! here: the runner is a stand-in that answers as a host would.

use std::cell::RefCell;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use client_core::SessionKey;
use client_core::files::{LocalTime, Made, local_time, locate_argv, made_argv, read_argv};
use desktop::draw::Rect;
use desktop::links::FileLink;
use desktop::viewer::{
    self, COPY_TIMEOUT, Document, IMAGE_TIMEOUT, ImageBody, LOCATE_TIMEOUT, MADE_FAILED,
    MADE_LOADING, MADE_TIMEOUT, MAX_BYTES, MAX_IMAGE_BYTES, MAX_IMAGE_PIXELS, MAX_OUTSIDE_BYTES,
    MAX_SIDE, MadeBody, MadeRow, MadeState, NO_CLAUDE, NO_IMAGES, Opened, READ_TIMEOUT, SideBody,
    SideLook, SidePanel, TextBody, decode, expand, extension, opens_outside, place, when,
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
        "doc.pdf", "v.mkv", "s.flac", "x.tar.gz", "b.tgz", "r.docx", "p.odp", "i.svg", "t.tif",
        "t.tiff", "p.heic", "p.avif", "f.ico",
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
    // The pictures the panel shows itself.
    for image in ["a.png", "A.JPEG", "b.jpg", "c.gif", "d.webp", "e.BMP"] {
        assert!(!opens_outside(image, false), "{image}");
        assert!(viewer::is_image(image), "{image}");
    }
    assert!(!viewer::is_image("i.svg") && !viewer::is_image("png") && !viewer::is_image(".png"));
    assert_eq!(viewer::IMAGES, ["png", "jpg", "jpeg", "gif", "webp", "bmp"]);
    assert!(opens_outside("src", true), "a folder");
    assert!(
        opens_outside("shots.png", true),
        "a folder, whatever it is called"
    );
    assert_eq!(viewer::OUTSIDE.len(), 29);
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
    let host = Host::new("f\n500\n/w/shot.SVG", b"\x89PNG");
    assert_eq!(
        target(host.open(LOCAL, &link("shot.SVG", None), None)),
        "/w/shot.SVG"
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
    let host = Host::new("f\n4\n/srv/pics/it's a shot.tif", b"\x89PNG");
    let opened = host.open(
        "nas",
        &link("/srv/pics/it's a shot.tif", None),
        Some(&cache),
    );
    let copy = cache.join("nas").join("it's a shot.tif");
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
            read_argv("nas", "/srv/pics/it's a shot.tif", 25_000_001, LOCAL),
            COPY_TIMEOUT
        )
    );
    // The next file of that name from that host replaces the copy.
    let host = Host::new("f\n3\n/other/it's a shot.tif", b"new");
    host.open("nas", &link("/other/it's a shot.tif", None), Some(&cache))
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
    let host = Host::new("f\n4\n/srv/a.tif", b"\x89PNG");
    let said = host
        .open("nas", &link("/srv/a.tif", None), None)
        .unwrap_err();
    assert_eq!(said, "/srv/a.tif on nas: no cache directory to copy it to");
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

/// A panel's body of 400 x 400 pixels at (600, 20).
const AREA: Rect = Rect {
    x: 600.0,
    y: 20.0,
    w: 400.0,
    h: 400.0,
};

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
    let view = here.view(10, 40, AREA);
    assert_eq!(view.title, "/w/src/main.rs");
    let SideBody::Text { gutter, rows } = &view.body else {
        panic!("not text: {view:?}");
    };
    assert_eq!(*gutter, 2);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[1].text, "    b");
    assert!(rows[1].marked && !rows[0].marked);
    assert_eq!(
        (here.host.as_str(), here.path.as_str()),
        (LOCAL, "/w/src/main.rs")
    );

    let there = SidePanel::text(&document("nas"), LOCAL);
    assert_eq!(there.view(10, 40, AREA).title, "nas:/w/src/main.rs");
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
    let SideBody::Text { rows, .. } = long.view(10, 40, AREA).body else {
        panic!("not text");
    };
    assert_eq!(rows[0].number, 4);
    assert!(!here.scroll(3, 10));
}

// --- pictures ---------------------------------------------------------------------------------

/// A picture as a file's bytes; `pixel` gives each pixel's RGBA.
fn encoded(
    format: image::ImageFormat,
    width: u32,
    height: u32,
    pixel: impl Fn(u32, u32) -> [u8; 4],
) -> Vec<u8> {
    let picture = image::RgbaImage::from_fn(width, height, |x, y| image::Rgba(pixel(x, y)));
    let mut out = std::io::Cursor::new(Vec::new());
    match format {
        // JPEG has no alpha.
        image::ImageFormat::Jpeg => image::DynamicImage::ImageRgba8(picture)
            .into_rgb8()
            .write_to(&mut out, format),
        _ => picture.write_to(&mut out, format),
    }
    .expect("encoding the test's picture");
    out.into_inner()
}

fn png(width: u32, height: u32, pixel: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    encoded(image::ImageFormat::Png, width, height, pixel)
}

/// The CRC of a PNG chunk.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= *byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320 & (!(crc & 1)).wrapping_add(1));
        }
    }
    !crc
}

/// A PNG file that is only its header, which says `width` x `height`, and the start of its
/// data: 60 bytes, whatever the size it claims.
fn png_header_only(width: u32, height: u32) -> Vec<u8> {
    png_header(width, height, 8)
}

/// The same with `depth` bits a channel (8 or 16).
fn png_header(width: u32, height: u32, depth: u8) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |kind: &[u8; 4], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc32(&body).to_be_bytes());
    };
    let mut header = Vec::new();
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    // RGBA, deflate, no filter, not interlaced.
    header.extend_from_slice(&[depth, 6, 0, 0, 0]);
    chunk(b"IHDR", &header);
    chunk(b"IDAT", &[0x78, 0x9c, 0x63, 0x00]);
    out
}

fn at(picture: &viewer::Picture, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * picture.width + x) * 4) as usize;
    picture.rgba[i..i + 4].try_into().unwrap()
}

const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];

fn corners(x: u32, y: u32) -> [u8; 4] {
    match (x, y) {
        (0, 0) => RED,
        (2, 0) => GREEN,
        (0, 1) => BLUE,
        (2, 1) => WHITE,
        _ => [10, 20, 30, 128],
    }
}

#[test]
fn a_small_png_decodes_to_its_pixels() {
    let picture = decode(&png(3, 2, corners), MAX_SIDE).expect("a 3x2 png");
    assert_eq!((picture.width, picture.height), (3, 2));
    assert_eq!(picture.rgba.len(), 3 * 2 * 4);
    assert_eq!(at(&picture, 0, 0), RED);
    assert_eq!(at(&picture, 2, 0), GREEN);
    assert_eq!(at(&picture, 0, 1), BLUE);
    assert_eq!(at(&picture, 2, 1), WHITE);
    // Alpha is kept as it is, not multiplied in.
    assert_eq!(at(&picture, 1, 0), [10, 20, 30, 128]);
    assert_eq!(
        (MAX_IMAGE_BYTES, MAX_IMAGE_PIXELS, MAX_SIDE),
        (25_000_000, 50_000_000, 4096)
    );
}

#[test]
fn the_other_formats_decode_and_a_gif_gives_its_first_frame() {
    use image::ImageFormat::{Bmp, Gif, Jpeg, WebP};
    for format in [Bmp, Gif, WebP, Jpeg] {
        let bytes = encoded(format, 8, 4, |_, _| RED);
        let picture = decode(&bytes, MAX_SIDE).unwrap_or_else(|e| panic!("{format:?}: {e}"));
        assert_eq!((picture.width, picture.height), (8, 4), "{format:?}");
        let [r, g, b, a] = at(&picture, 4, 2);
        assert!(r > 240 && g < 15 && b < 15 && a == 255, "{format:?}");
    }
    // Two frames: the first is the one shown.
    let mut bytes = Vec::new();
    {
        let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
        for colour in [GREEN, BLUE] {
            let frame = image::RgbaImage::from_pixel(5, 5, image::Rgba(colour));
            encoder
                .encode_frame(image::Frame::new(frame))
                .expect("a gif frame");
        }
    }
    let picture = decode(&bytes, MAX_SIDE).expect("a gif of two frames");
    assert_eq!((picture.width, picture.height), (5, 5));
    assert_eq!(at(&picture, 2, 2), GREEN);
}

#[test]
fn a_picture_wider_than_the_limit_is_made_smaller_with_its_shape_kept() {
    let picture = decode(&png(6000, 10, |_, _| BLUE), MAX_SIDE).expect("a 6000x10 png");
    assert_eq!(picture.width, 4096);
    // 10 * 4096 / 6000 = 6.83
    assert_eq!(picture.height, 7);
    assert_eq!(picture.rgba.len(), 4096 * 7 * 4);
    assert_eq!(at(&picture, 2000, 3), BLUE);
    // Taller than wide, and a limit of the caller's own.
    let picture = decode(&png(10, 300, |_, _| BLUE), 100).expect("a 10x300 png");
    assert_eq!((picture.width, picture.height), (3, 100));
    // Never down to nothing.
    let picture = decode(&png(1, 3000, |_, _| BLUE), 100).expect("a 1x3000 png");
    assert_eq!((picture.width, picture.height), (1, 100));
    // One that fits is left as it is.
    let picture = decode(&png(100, 100, |_, _| BLUE), 100).expect("a 100x100 png");
    assert_eq!((picture.width, picture.height), (100, 100));
}

#[test]
fn what_is_not_a_picture_is_said_not_shown() {
    let said = decode(b"just some text, called shot.png\n", MAX_SIDE).unwrap_err();
    assert!(said.contains("not an image"), "{said}");
    let said = decode(b"", MAX_SIDE).unwrap_err();
    assert!(said.contains("not an image"), "{said}");
    // A format the panel does not show (TIFF's first bytes).
    let said = decode(b"II*\0\x08\0\0\0\0\0\0\0\0\0\0\0", MAX_SIDE).unwrap_err();
    assert!(said.contains("not an image"), "{said}");
}

#[test]
fn a_header_that_claims_too_many_pixels_is_refused_before_anything_is_decoded() {
    // 60 bytes that say 20000 x 20000 (1.6 GB as RGBA). Decoding them would fail for the
    // missing data with another message: this one comes from the header alone.
    let bytes = png_header_only(20_000, 20_000);
    assert!(bytes.len() < 100);
    assert_eq!(decode(&bytes, MAX_SIDE), Err("image too large".to_string()));
    // The same header with a size that is allowed gets as far as the data, which is not there.
    let said = decode(&png_header_only(20, 20), MAX_SIDE).unwrap_err();
    assert!(said.starts_with("broken image"), "{said}");
    // Width times height does not fit 32 bits: no overflow.
    assert_eq!(
        decode(&png_header_only(0x7fff_ffff, 0x7fff_ffff), MAX_SIDE),
        Err("image too large".to_string())
    );
}

#[test]
fn a_cut_off_picture_is_an_error_not_a_panic() {
    let whole = encoded(image::ImageFormat::Jpeg, 64, 48, |x, y| {
        [x as u8 * 4, y as u8 * 5, 90, 255]
    });
    assert!(decode(&whole, MAX_SIDE).is_ok());
    // Cut inside its tables: nothing of it can be shown.
    let said = decode(&whole[..40], MAX_SIDE).unwrap_err();
    assert!(said.starts_with("broken image"), "{said}");
    // Cut anywhere else: an error or as much of it as there is, never a panic.
    for end in (2..whole.len()).step_by(7) {
        if let Ok(picture) = decode(&whole[..end], MAX_SIDE) {
            assert_eq!((picture.width, picture.height), (64, 48));
        }
    }
    // A PNG that lacks more than its last chunks has not all its rows: an error.
    let png = png(40, 40, |x, y| {
        [x as u8 * 6, y as u8 * 6, (x * y) as u8, 255]
    });
    for end in 0..png.len() {
        let cut = decode(&png[..end], MAX_SIDE);
        assert!(cut.is_err() || end + 40 > png.len(), "png cut at {end}");
    }
}

fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect { x, y, w, h }
}

#[test]
fn a_picture_is_fitted_into_the_panel_and_never_enlarged() {
    // Smaller than the panel: as it is, in the middle.
    let (dest, source) = place((100, 50), AREA, false, (0.0, 0.0));
    assert_eq!(dest, rect(750.0, 195.0, 100.0, 50.0));
    assert_eq!(source, rect(0.0, 0.0, 100.0, 50.0));
    // Larger: as wide as the panel, its shape kept.
    let (dest, source) = place((1000, 500), AREA, false, (0.0, 0.0));
    assert_eq!(dest, rect(600.0, 120.0, 400.0, 200.0));
    assert_eq!(source, rect(0.0, 0.0, 1000.0, 500.0));
    // Taller than wide: as high as the panel.
    let (dest, _) = place((500, 1000), AREA, false, (0.0, 0.0));
    assert_eq!(dest, rect(700.0, 20.0, 200.0, 400.0));
    // Scrolling means nothing while it is fitted.
    assert_eq!(
        place((1000, 500), AREA, false, (300.0, 300.0)),
        place((1000, 500), AREA, false, (0.0, 0.0))
    );
    // Whatever the panel's size the shape is the picture's, within a pixel.
    for (w, h) in [(333.0, 77.0), (51.0, 400.0), (1.0, 1.0), (640.5, 480.25)] {
        let (dest, _) = place((1280, 800), rect(3.0, 4.0, w, h), false, (0.0, 0.0));
        assert!(dest.w <= w && dest.h <= h, "{dest:?} in {w}x{h}");
        assert!(dest.x >= 3.0 && dest.y >= 4.0, "{dest:?}");
        assert!((dest.w / 1.6 - dest.h).abs() <= 1.0, "{dest:?} in {w}x{h}");
    }
}

#[test]
fn at_full_size_the_panel_shows_the_part_scrolled_to() {
    // The panel's size in texels, from the top-left corner.
    let (dest, source) = place((1000, 900), AREA, true, (0.0, 0.0));
    assert_eq!(dest, AREA);
    assert_eq!(source, rect(0.0, 0.0, 400.0, 400.0));
    let (dest, source) = place((1000, 900), AREA, true, (250.0, 100.0));
    assert_eq!(dest, AREA);
    assert_eq!(source, rect(250.0, 100.0, 400.0, 400.0));
    // Past either end: the picture's edge stays at the panel's.
    let (_, source) = place((1000, 900), AREA, true, (5000.0, 5000.0));
    assert_eq!(source, rect(600.0, 500.0, 400.0, 400.0));
    let (_, source) = place((1000, 900), AREA, true, (-70.0, -1.0));
    assert_eq!(source, rect(0.0, 0.0, 400.0, 400.0));
    // Wider than the panel and less high: scrolled sideways only, centred the other way.
    let (dest, source) = place((1000, 100), AREA, true, (30.0, 30.0));
    assert_eq!(dest, rect(600.0, 170.0, 400.0, 100.0));
    assert_eq!(source, rect(30.0, 0.0, 400.0, 100.0));
    // One texel is one pixel: both have the same size.
    for scroll in [(0.0, 0.0), (17.5, 3.25), (1e9, -1e9)] {
        let (dest, source) = place((1000, 100), rect(0.5, 0.5, 333.3, 77.7), true, scroll);
        assert_eq!((dest.w, dest.h), (source.w, source.h));
        assert!(source.x >= 0.0 && source.x + source.w <= 1000.0);
        assert!(source.y >= 0.0 && source.y + source.h <= 100.0);
    }
}

#[test]
fn a_panel_or_a_picture_of_no_size_draws_nothing() {
    for actual in [false, true] {
        for area in [
            rect(600.0, 20.0, 0.0, 0.0),
            rect(600.0, 20.0, 0.0, 400.0),
            rect(600.0, 20.0, 400.0, 0.0),
            rect(600.0, 20.0, -5.0, 400.0),
        ] {
            let (dest, source) = place((100, 50), area, actual, (10.0, 10.0));
            assert_eq!((dest.w, dest.h), (0.0, 0.0), "{area:?}");
            for value in [dest.x, dest.y, source.x, source.y, source.w, source.h] {
                assert!(value.is_finite(), "{area:?}: {dest:?} {source:?}");
            }
        }
        let (dest, _) = place((0, 0), AREA, actual, (0.0, 0.0));
        assert_eq!((dest.w, dest.h), (0.0, 0.0));
    }
}

#[test]
fn a_png_of_this_machine_is_read_and_decoded_for_the_panel() {
    let bytes = png(3, 2, corners);
    let host = Host::new(&format!("f\n{}\n/w/shot.png", bytes.len()), &bytes);
    let opened = host.open(LOCAL, &link("shot.png", Some(3)), None);
    let Ok(Opened::Image {
        host: on,
        path,
        file,
        picture,
    }) = opened
    else {
        panic!("not a picture: {opened:?}");
    };
    assert_eq!((on.as_str(), path.as_str()), (LOCAL, "/w/shot.png"));
    assert_eq!(file, (3, 2));
    assert_eq!((picture.width, picture.height), (3, 2));
    assert_eq!(at(&picture, 2, 1), WHITE);
    // One byte more than the limit is asked for, with the time a copy gets.
    let calls = host.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(
        calls[1],
        (
            read_argv(LOCAL, "/w/shot.png", MAX_IMAGE_BYTES + 1, LOCAL),
            IMAGE_TIMEOUT
        )
    );
    assert_eq!(IMAGE_TIMEOUT, Duration::from_secs(60));

    // From another host: through ssh, and nothing is written here.
    let host = Host::new(&format!("f\n{}\n/w/Shot.JPG", bytes.len()), &bytes);
    let opened = host.open("nas", &link("/w/Shot.JPG", None), None);
    assert!(
        matches!(&opened, Ok(Opened::Image { host, path, .. }) if host == "nas" && path == "/w/Shot.JPG"),
        "{opened:?}"
    );
    assert_eq!(
        host.calls()[1].0,
        read_argv("nas", "/w/Shot.JPG", MAX_IMAGE_BYTES + 1, LOCAL)
    );
}

#[test]
fn a_picture_the_panel_refuses_goes_to_the_desktops_app_and_the_line_says_why() {
    let outside = |opened: Result<Opened, String>| match opened {
        Ok(Opened::Outside { target, said }) => (target, said),
        other => panic!("not for the desktop's app: {other:?}"),
    };
    // 100 MB by what the host says: nothing is read, and it opens where it is.
    let host = Host::new("f\n100000000\n/w/huge.png", b"");
    assert_eq!(
        outside(host.open(LOCAL, &link("/w/huge.png", None), None)),
        (
            "/w/huge.png".to_string(),
            "too large for the panel; opened /w/huge.png in its app".to_string()
        )
    );
    assert_eq!(host.calls().len(), 1, "located, not read");
    // On another host it is too large to copy as well: the line there was before.
    let host = Host::new("f\n100000000\n/w/huge.png", b"");
    assert_eq!(
        host.open("nas", &link("/w/huge.png", None), None),
        Err("/w/huge.png on nas: too large to copy (100.0 MB, the limit is 25 MB)".to_string())
    );
    assert_eq!(host.calls().len(), 1);
    // It grew since it was located: read up to the limit and a byte, not decoded.
    let grown = vec![0u8; MAX_IMAGE_BYTES as usize + 5];
    let host = Host::new("f\n10\n/w/grown.png", &grown);
    assert_eq!(
        outside(host.open(LOCAL, &link("/w/grown.png", None), None)).1,
        "too large for the panel; opened /w/grown.png in its app"
    );
    // Text called .png.
    let host = Host::new("f\n6\n/w/notes.png", b"hello\n");
    assert_eq!(
        outside(host.open(LOCAL, &link("/w/notes.png", None), None)),
        (
            "/w/notes.png".to_string(),
            "not a picture the panel shows; opened /w/notes.png in its app".to_string()
        )
    );
    // The same from another host: the bytes read for the panel are the copy.
    let cache = scratch("refused-picture");
    let host = Host::new("f\n6\n/w/notes.png", b"hello\n");
    let (target, said) = outside(host.open("nas", &link("/w/notes.png", None), Some(&cache)));
    assert_eq!(target, cache.join("nas/notes.png").to_str().unwrap());
    assert_eq!(std::fs::read(&target).unwrap(), b"hello\n");
    assert!(
        said.starts_with("not a picture the panel shows; copied /w/notes.png on nas to "),
        "{said}"
    );
    assert_eq!(host.calls().len(), 2, "not fetched a second time");
    let _ = std::fs::remove_dir_all(&cache);
    // A header that claims 20000 x 20000, and one whose 16 bits a channel are too many bytes.
    for bytes in [png_header_only(20_000, 20_000), png_header(5100, 5000, 16)] {
        let host = Host::new(&format!("f\n{}\n/w/big.png", bytes.len()), &bytes);
        assert_eq!(
            outside(host.open(LOCAL, &link("/w/big.png", None), None)).1,
            "too large for the panel; opened /w/big.png in its app"
        );
    }
    // A JPEG cut off.
    let whole = encoded(image::ImageFormat::Jpeg, 64, 48, |_, _| RED);
    let host = Host::new("f\n40\n/w/cut.jpeg", &whole[..40]);
    let said = host
        .open(LOCAL, &link("/w/cut.jpeg", None), None)
        .unwrap_err();
    assert!(said.starts_with("/w/cut.jpeg: broken image"), "{said}");
    assert_eq!(said.lines().count(), 1, "{said}");
}

#[test]
fn a_link_that_takes_too_long_says_so_and_names_the_picture() {
    let slow = |argv: &[String], timeout: Duration| -> anyhow::Result<Vec<u8>> {
        match argv.last().unwrap().contains("head -c") {
            false => Ok(b"f\n500\n/w/shot.png".to_vec()),
            true => anyhow::bail!("ssh timed out after {:.1} s", timeout.as_secs_f64()),
        }
    };
    let said = viewer::open_link(&key("nas"), &link("/w/shot.png", None), LOCAL, None, &slow);
    assert_eq!(
        said,
        Err("/w/shot.png on nas: ssh timed out after 60.0 s".to_string())
    );
}

#[test]
fn what_the_panel_does_not_show_still_goes_to_the_desktops_app() {
    for name in ["/w/doc.pdf", "/w/draw.svg", "/w/scan.tif", "/w/p.heic"] {
        let host = Host::new(&format!("f\n500\n{name}"), b"%PDF-1.7");
        let opened = host.open(LOCAL, &link(name, None), None);
        assert_eq!(target(opened), name);
        assert_eq!(host.calls().len(), 1, "{name}: not read on this machine");
    }
    // A folder called like a picture is a folder.
    let host = Host::new("d\n0\n/w/shots.png", b"");
    assert_eq!(
        target(host.open(LOCAL, &link("/w/shots.png", None), None)),
        "/w/shots.png"
    );
}

#[test]
fn a_picture_panel_toggles_between_fitted_and_full_size_and_scrolls_only_then() {
    let mut panel = SidePanel::image("nas", "/w/shot.png", (1000, 900), (1000, 900), LOCAL);
    assert_eq!(
        (panel.host.as_str(), panel.path.as_str()),
        ("nas", "/w/shot.png")
    );
    let view = panel.view(10, 40, AREA);
    assert_eq!(view.body, SideBody::Image);
    assert_eq!(view.title, "nas:/w/shot.png 1000×900 · 40 %");
    assert_eq!(
        panel.image_body(),
        Some(&ImageBody {
            size: (1000, 900),
            file: (1000, 900),
            actual: false,
            scroll: (0.0, 0.0)
        })
    );
    // Fitted: the wheel moves nothing.
    assert!(!panel.pan((0.0, 51.0), AREA));
    // A click: full size, from the top-left corner.
    assert!(panel.click());
    assert_eq!(
        panel.view(10, 40, AREA).title,
        "nas:/w/shot.png 1000×900 · 100 %"
    );
    assert!(panel.pan((0.0, 51.0), AREA));
    assert!(panel.pan((34.0, 0.0), AREA));
    assert_eq!(panel.image_body().unwrap().scroll, (34.0, 51.0));
    // To the end and no further; at the end nothing changes any more.
    assert!(panel.pan((9000.0, 9000.0), AREA));
    assert_eq!(panel.image_body().unwrap().scroll, (600.0, 500.0));
    assert!(!panel.pan((10.0, 10.0), AREA));
    assert!(panel.pan((-9000.0, -20.0), AREA));
    assert_eq!(panel.image_body().unwrap().scroll, (0.0, 480.0));
    // The panel got larger since: less is left to scroll, and it is at the end of that.
    let large = rect(0.0, 0.0, 800.0, 800.0);
    assert!(!panel.pan((0.0, 10.0), large));
    assert_eq!(panel.image_body().unwrap().scroll, (0.0, 100.0));
    assert!(panel.pan((0.0, -10.0), large));
    assert_eq!(panel.image_body().unwrap().scroll, (0.0, 90.0));
    // A click again: fitted, and the next full size starts at the corner.
    assert!(panel.click());
    assert_eq!(
        panel.image_body().unwrap(),
        &ImageBody {
            size: (1000, 900),
            file: (1000, 900),
            actual: false,
            scroll: (0.0, 0.0)
        }
    );
    // The text's wheel is not the picture's, and a click is not the text's.
    assert!(!panel.scroll(3, 10));
    let mut text = SidePanel::text(
        &Document {
            host: LOCAL.to_string(),
            path: "/w/a.rs".to_string(),
            line: None,
            text: numbered(50),
        },
        LOCAL,
    );
    assert!(!text.click() && !text.pan((0.0, 40.0), AREA) && text.image_body().is_none());

    // A picture smaller than the panel is at full size already.
    let small = SidePanel::image(LOCAL, "/w/dot.png", (40, 30), (40, 30), LOCAL);
    assert_eq!(small.view(10, 40, AREA).title, "/w/dot.png 40×30 · 100 %");
    // A panel of no size: no division by zero.
    let none = rect(0.0, 0.0, 0.0, 0.0);
    assert_eq!(
        panel.view(0, 0, none).title,
        "nas:/w/shot.png 1000×900 · 0 %"
    );
}

#[test]
fn sixteen_bits_a_channel_count_as_the_bytes_they_take() {
    // 25.5 million pixels are allowed; as RGBA of 16 bits they are 204 MB, more than the
    // 200 MB a decode may ask for. Refused by the header, before the buffer.
    assert_eq!(
        decode(&png_header(5100, 5000, 16), MAX_SIDE),
        Err("image too large".to_string())
    );
    // With 8 bits it gets as far as the data, which is not there.
    let said = decode(&png_header(5100, 5000, 8), MAX_SIDE).unwrap_err();
    assert!(said.starts_with("broken image"), "{said}");
    assert_eq!(
        viewer::decode_file(&png_header(5100, 5000, 16), MAX_SIDE).map(|_| ()),
        Err(viewer::Refused::TooLarge)
    );
    assert_eq!(
        viewer::decode_file(b"text", MAX_SIDE).map(|_| ()),
        Err(viewer::Refused::NotAnImage)
    );
}

#[test]
fn small_pictures_of_other_pixel_kinds_come_out_as_rgba_bytes() {
    let file = |picture: image::DynamicImage| {
        let mut out = std::io::Cursor::new(Vec::new());
        picture
            .write_to(&mut out, image::ImageFormat::Png)
            .expect("encoding the test's picture");
        out.into_inner()
    };
    // 16 bits a channel, with alpha.
    let deep = image::ImageBuffer::from_pixel(3, 2, image::Rgba([0xffffu16, 0, 0x8080, 0x8080]));
    let picture = decode(&file(image::DynamicImage::ImageRgba16(deep)), MAX_SIDE).unwrap();
    assert_eq!((picture.width, picture.height), (3, 2));
    assert_eq!(picture.rgba.len(), 3 * 2 * 4);
    assert_eq!(at(&picture, 2, 1), [255, 0, 128, 128]);
    // No alpha: opaque.
    let plain = image::ImageBuffer::from_pixel(2, 2, image::Rgb([1u8, 2, 3]));
    let picture = decode(&file(image::DynamicImage::ImageRgb8(plain)), MAX_SIDE).unwrap();
    assert_eq!(picture.rgba.len(), 2 * 2 * 4);
    assert_eq!(at(&picture, 1, 1), [1, 2, 3, 255]);
    // Grey of 16 bits.
    let grey = image::ImageBuffer::from_pixel(2, 2, image::Luma([0x4040u16]));
    let picture = decode(&file(image::DynamicImage::ImageLuma16(grey)), MAX_SIDE).unwrap();
    assert_eq!(at(&picture, 0, 0), [64, 64, 64, 255]);
    // 16 bits made smaller: still four bytes a pixel.
    let wide = image::ImageBuffer::from_pixel(300, 10, image::Rgba([0u16, 0xffff, 0, 0xffff]));
    let picture = decode(&file(image::DynamicImage::ImageRgba16(wide)), 100).unwrap();
    assert_eq!((picture.width, picture.height), (100, 3));
    assert_eq!(at(&picture, 50, 1), GREEN);
}

#[test]
fn a_picture_made_smaller_is_still_called_by_the_files_size() {
    let decoded = viewer::decode_file(&png(6000, 10, |_, _| BLUE), MAX_SIDE).expect("a png");
    assert_eq!(decoded.file, (6000, 10));
    assert_eq!((decoded.picture.width, decoded.picture.height), (4096, 7));
    // Through a click: the panel's header says the file's pixels, and how large one of those
    // is drawn. 400 of the body's pixels for 6000: 7 %. At the texture's own size: 68 %.
    let bytes = png(6000, 10, |_, _| BLUE);
    let host = Host::new(&format!("f\n{}\n/w/wide.png", bytes.len()), &bytes);
    let Ok(Opened::Image { file, picture, .. }) =
        host.open(LOCAL, &link("/w/wide.png", None), None)
    else {
        panic!("not a picture");
    };
    assert_eq!(file, (6000, 10));
    let size = (picture.width, picture.height);
    let mut panel = SidePanel::image(LOCAL, "/w/wide.png", size, file, LOCAL);
    assert_eq!(panel.view(10, 40, AREA).title, "/w/wide.png 6000×10 · 7 %");
    assert!(panel.click());
    assert_eq!(panel.view(10, 40, AREA).title, "/w/wide.png 6000×10 · 68 %");
    // What is placed and scrolled is the texture.
    assert_eq!(panel.image_body().unwrap().size, (4096, 7));
    assert!(panel.pan((1e9, 0.0), AREA));
    assert_eq!(panel.image_body().unwrap().scroll, (3696.0, 0.0));
}

#[test]
fn a_click_that_is_no_longer_the_latest_is_not_decoded() {
    let bytes = png(3, 2, corners);
    let host = Host::new(&format!("f\n{}\n/w/shot.png", bytes.len()), &bytes);
    let run = |argv: &[String], timeout: Duration| host.run(argv, timeout);
    let asked = std::cell::Cell::new(0);
    let stale = || {
        asked.set(asked.get() + 1);
        false
    };
    let opened = viewer::open_link_while(
        &key(LOCAL),
        &link("/w/shot.png", None),
        LOCAL,
        None,
        &run,
        &stale,
    );
    assert_eq!(
        opened,
        Err("/w/shot.png: a newer click took its place".to_string())
    );
    assert_eq!(host.calls().len(), 2, "read, then asked");
    assert_eq!(asked.get(), 1);
    // Still the latest: decoded.
    let opened = viewer::open_link_while(
        &key(LOCAL),
        &link("/w/shot.png", None),
        LOCAL,
        None,
        &run,
        &|| true,
    );
    assert!(matches!(opened, Ok(Opened::Image { .. })), "{opened:?}");
}

#[test]
fn work_that_panics_is_an_answer_too() {
    let quiet = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let answer = viewer::guarded(|| panic!("a decoder gave up"));
    std::panic::set_hook(quiet);
    assert_eq!(answer, Err("broken image".to_string()));
    let fine = viewer::guarded(|| Err("as it was".to_string()));
    assert_eq!(fine, Err("as it was".to_string()));
}

// --- the images a session made ---------------------------------------------------------------

const ID: &str = "0b5c1d2e-3f4a-4b6c-8d7e-9f0a1b2c3d4e";

fn file(path: &str, modified_s: u64) -> Made {
    Made {
        path: path.to_string(),
        modified_s,
        bytes: 10,
    }
}

/// `count` files `/w/plots/N.png`, the newest (N = 0) first.
fn files(count: usize) -> Vec<Made> {
    (0..count)
        .map(|n| file(&format!("/w/plots/{n}.png"), 1_760_000_000 - n as u64))
        .collect()
}

fn names(body: &MadeBody, visible: usize) -> Vec<String> {
    body.rows(visible, 60, 1_760_000_000)
        .into_iter()
        .map(|row| row.name)
        .collect()
}

fn selected(body: &MadeBody, visible: usize) -> Vec<String> {
    body.rows(visible, 60, 1_760_000_000)
        .into_iter()
        .filter(|row| row.selected)
        .map(|row| row.name)
        .collect()
}

#[test]
fn the_list_is_asked_of_the_sessions_host_within_the_time_limit() {
    let out = "1760000300\t12\t/w/plots/c.png\n1760000200\t3456\t/w/b.jpg\n";
    for host in [LOCAL, "nas"] {
        let calls = RefCell::new(Vec::new());
        let run = |argv: &[String], timeout: Duration| {
            calls.borrow_mut().push((argv.to_vec(), timeout));
            Ok(out.as_bytes().to_vec())
        };
        let opened = viewer::made(&key(host), ID, LOCAL, &run).unwrap();
        assert_eq!(
            opened,
            Opened::Made {
                host: host.to_string(),
                session: "work".to_string(),
                files: vec![
                    Made {
                        path: "/w/plots/c.png".to_string(),
                        modified_s: 1_760_000_300,
                        bytes: 12
                    },
                    Made {
                        path: "/w/b.jpg".to_string(),
                        modified_s: 1_760_000_200,
                        bytes: 3456
                    },
                ],
            }
        );
        // One command: the one `client_core::files` builds for that id, and nothing else.
        let calls = calls.into_inner();
        assert_eq!(
            calls,
            vec![(made_argv(host, ID, None, LOCAL), MADE_TIMEOUT)]
        );
        assert_eq!(calls[0].0[0], if host == LOCAL { "sh" } else { "ssh" });
    }
    assert_eq!(
        MADE_TIMEOUT, READ_TIMEOUT,
        "as long as a file's text may take"
    );
}

#[test]
fn a_session_without_a_transcript_has_no_images_and_a_dead_link_says_so() {
    let none = |_: &[String], _: Duration| Ok(b"no-transcript\n".to_vec());
    let opened = viewer::made(&key("nas"), ID, LOCAL, &none).unwrap();
    assert_eq!(
        opened,
        Opened::Made {
            host: "nas".to_string(),
            session: "work".to_string(),
            files: Vec::new()
        }
    );
    // What `tmux::run_bytes` says of a command it had to kill.
    let dead = |_: &[String], _: Duration| Err(anyhow::anyhow!("ssh timed out after 10.0 s"));
    assert_eq!(
        viewer::made(&key("nas"), ID, LOCAL, &dead),
        Err("images of work on nas: ssh timed out after 10.0 s".to_string())
    );
    let refused = |_: &[String], _: Duration| {
        Err(anyhow::anyhow!(
            "ssh: connect to host nas port 22: Connection refused\nmore"
        ))
    };
    assert_eq!(
        viewer::made(&key(LOCAL), ID, LOCAL, &refused),
        Err("images of work: ssh: connect to host nas port 22: Connection refused".to_string())
    );
}

#[test]
fn an_id_that_is_none_never_reaches_a_command() {
    let seen = RefCell::new(Vec::new());
    let run = |argv: &[String], _: Duration| {
        seen.borrow_mut().push(argv.to_vec());
        Ok(b"no-transcript\n".to_vec())
    };
    let opened = viewer::made(&key("nas"), "$(reboot)", LOCAL, &run).unwrap();
    assert!(matches!(opened, Opened::Made { files, .. } if files.is_empty()));
    for argv in seen.into_inner() {
        assert!(argv.iter().all(|word| !word.contains("reboot")), "{argv:?}");
    }
}

#[test]
fn the_rows_are_in_the_lists_order_with_name_and_folder_apart() {
    let listed = vec![
        file("/w/plots/c.png", 1_760_000_300),
        file(
            "/home/someone/projects/weather/output/figures/b.jpg",
            1_760_000_200,
        ),
        file("/a.webp", 1_760_000_100),
    ];
    let body = MadeBody::listed(key(LOCAL), listed);
    assert_eq!(body.state(), MadeState::Listed);
    assert_eq!(body.len(), 3);
    assert_eq!(body.message(), None);
    let rows = body.rows(10, 40, 1_760_000_400);
    let shown: Vec<(&str, &str, bool)> = rows
        .iter()
        .map(|row| (row.name.as_str(), row.folder.as_str(), row.selected))
        .collect();
    // 40 columns: seven for the time, the name, two apart, the folder in what is left (26).
    assert_eq!(
        shown,
        [
            ("c.png", "/w/plots", true),
            ("b.jpg", "…ts/weather/output/figures", false),
            ("a.webp", "/", false),
        ]
    );
    assert_eq!(width(&rows[1].folder), 26);
    // Too narrow for a folder worth reading: the name alone.
    let rows = body.rows(10, 15, 1_760_000_400);
    assert_eq!(
        (rows[0].name.as_str(), rows[0].folder.as_str()),
        ("c.png", "")
    );
    // Only what is in view.
    assert_eq!(names(&body, 2), ["c.png", "b.jpg"]);
    assert_eq!(names(&body, 0), Vec::<String>::new());
}

#[test]
fn a_row_says_the_time_of_today_and_the_day_of_any_other() {
    let at = |month, day, hour, minute| LocalTime {
        year: 2026,
        month,
        day,
        hour,
        minute,
    };
    let now = at(10, 11, 14, 5);
    assert_eq!(when(at(10, 11, 9, 7), now), "09:07");
    assert_eq!(when(at(10, 11, 0, 0), now), "00:00");
    assert_eq!(when(at(10, 10, 23, 59), now), "Oct 10");
    assert_eq!(when(at(1, 3, 12, 0), now), "Jan 3");
    assert_eq!(when(at(12, 31, 12, 0), now), "Dec 31");
    // The same day of another year is not today.
    let last_year = LocalTime { year: 2025, ..now };
    assert_eq!(when(last_year, now), "Oct 11");
    // A month no calendar has does not panic.
    assert_eq!(when(at(13, 1, 0, 0), now), "? 1");

    // On this machine's clock: a file of this very moment is of today, one of 40 days ago not.
    let now_s = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let body = MadeBody::listed(
        key(LOCAL),
        vec![
            file("/w/new.png", now_s),
            file("/w/old.png", now_s - 40 * 86_400),
        ],
    );
    let rows = body.rows(5, 60, now_s);
    let clock = local_time(now_s).expect("this machine's time");
    assert_eq!(
        rows[0].when,
        format!("{:02}:{:02}", clock.hour, clock.minute)
    );
    let old = local_time(now_s - 40 * 86_400).unwrap();
    assert!(
        rows[1].when.ends_with(&format!(" {}", old.day)),
        "{:?}",
        rows[1].when
    );
    assert!(rows.iter().all(|row| width(&row.when) <= 6), "{rows:?}");
}

#[test]
fn the_cursor_stops_at_both_ends_and_stays_in_view() {
    let mut body = MadeBody::listed(key(LOCAL), files(10));
    assert_eq!(selected(&body, 4), ["0.png"]);
    assert_eq!(
        body.selected().map(|file| file.path.as_str()),
        Some("/w/plots/0.png")
    );
    assert!(!body.move_by(-1, 4), "already on the first");
    assert!(!body.move_by(-100, 4));
    assert_eq!(selected(&body, 4), ["0.png"]);
    assert!(body.move_by(1, 4));
    assert_eq!(selected(&body, 4), ["1.png"]);
    assert_eq!(names(&body, 4), ["0.png", "1.png", "2.png", "3.png"]);
    // Past the last line in view: the list follows.
    assert!(body.move_by(3, 4));
    assert_eq!(names(&body, 4), ["1.png", "2.png", "3.png", "4.png"]);
    assert_eq!(selected(&body, 4), ["4.png"]);
    assert!(body.move_by(100, 4));
    assert_eq!(names(&body, 4), ["6.png", "7.png", "8.png", "9.png"]);
    assert_eq!(selected(&body, 4), ["9.png"]);
    assert!(!body.move_by(1, 4), "already on the last");
    assert!(body.move_by(-9, 4));
    assert_eq!(names(&body, 4), ["0.png", "1.png", "2.png", "3.png"]);
    // No room at all, or nothing listed: nothing to divide by, nothing moves.
    assert!(body.move_by(1, 0));
    let mut empty = MadeBody::listed(key(LOCAL), Vec::new());
    assert!(!empty.move_by(1, 4));
    assert!(!empty.move_by(-1, 0));
    assert_eq!(empty.selected(), None);
}

#[test]
fn the_wheel_scrolls_the_list_between_its_ends() {
    let mut body = MadeBody::listed(key(LOCAL), files(10));
    assert!(!body.scroll(-3, 4), "at the top");
    assert!(body.scroll(3, 4));
    assert_eq!(names(&body, 4), ["3.png", "4.png", "5.png", "6.png"]);
    // The cursor stays on its file, in view or not.
    assert_eq!(
        body.selected().map(|file| file.path.as_str()),
        Some("/w/plots/0.png")
    );
    assert_eq!(selected(&body, 4), Vec::<String>::new());
    assert!(body.scroll(100, 4));
    assert_eq!(names(&body, 4), ["6.png", "7.png", "8.png", "9.png"]);
    assert!(!body.scroll(1, 4), "at the end");
    // A taller panel: the end stays at its foot.
    assert_eq!(names(&body, 8).len(), 8);
    assert_eq!(names(&body, 8)[7], "9.png");
    assert_eq!(names(&body, 30).len(), 10);
    assert!(!body.scroll(5, 30), "all of it is in view");
    // A click takes the row under it; under the last row there is none.
    body.scroll(-100, 4);
    assert_eq!(
        body.click(2, 4).map(|file| file.path.as_str()),
        Some("/w/plots/2.png")
    );
    assert_eq!(selected(&body, 4), ["2.png"]);
    assert_eq!(body.click(4, 4), None, "under the panel");
    let mut short = MadeBody::listed(key(LOCAL), files(2));
    assert_eq!(short.click(2, 10), None);
    assert_eq!(
        short.click(1, 10).map(|file| file.path.as_str()),
        Some("/w/plots/1.png")
    );
}

#[test]
fn what_is_not_a_list_is_one_line() {
    let empty = MadeBody::listed(key(LOCAL), Vec::new());
    assert_eq!(NO_IMAGES, "no images from this session yet");
    assert_eq!(empty.message(), Some(NO_IMAGES));
    assert_eq!(empty.rows(10, 60, 0), Vec::<MadeRow>::new());
    let none = MadeBody::no_claude(key(LOCAL));
    assert_eq!(NO_CLAUDE, "no Claude session in this tab");
    assert_eq!(
        (none.state(), none.message()),
        (MadeState::NoClaude, Some(NO_CLAUDE))
    );
    let mut loading = MadeBody::loading(key(LOCAL));
    assert_eq!(
        (loading.state(), loading.message()),
        (MadeState::Loading, Some(MADE_LOADING))
    );
    // The host did not answer: said, and a later answer is still taken.
    loading.failed();
    assert_eq!(
        (loading.state(), loading.message()),
        (MadeState::Failed, Some(MADE_FAILED))
    );
    loading.set_files(files(2));
    assert_eq!(
        (loading.state(), loading.message()),
        (MadeState::Listed, None)
    );

    // As the panel draws them: the line, and no rows.
    for (body, line, title) in [
        (empty, NO_IMAGES, "Images · work · 0"),
        (none, NO_CLAUDE, "Images · work"),
        (MadeBody::loading(key(LOCAL)), MADE_LOADING, "Images · work"),
    ] {
        let view = SidePanel::made(body).view(20, 60, AREA);
        assert_eq!(view.title, title);
        assert_eq!(
            view.body,
            SideBody::Made {
                rows: Vec::new(),
                message: Some(line)
            }
        );
    }
}

#[test]
fn a_list_that_is_read_again_keeps_the_cursor_on_its_file() {
    let mut body = MadeBody::listed(key(LOCAL), files(6));
    body.move_by(3, 4);
    assert_eq!(selected(&body, 4), ["3.png"]);
    // Two new files came first.
    let mut newer = vec![
        file("/w/new-a.png", 1_760_000_900),
        file("/w/new-b.png", 1_760_000_800),
    ];
    newer.extend(files(6));
    body.set_files(newer);
    assert_eq!(body.len(), 8);
    assert_eq!(
        body.selected().map(|file| file.path.as_str()),
        Some("/w/plots/3.png")
    );
    assert_eq!(selected(&body, 4), ["3.png"], "and in view");
    // Its file is gone: the cursor stays on its line, or the last there is.
    body.set_files(files(2));
    assert_eq!(
        body.selected().map(|file| file.path.as_str()),
        Some("/w/plots/1.png")
    );
    body.set_files(Vec::new());
    assert_eq!((body.selected(), body.message()), (None, Some(NO_IMAGES)));
    // A failure does not take a list away that is there.
    let mut kept = MadeBody::listed(key(LOCAL), files(2));
    kept.failed();
    assert_eq!((kept.state(), kept.len()), (MadeState::Listed, 2));
}

#[test]
fn the_lists_panel_says_whose_images_and_how_many_and_a_picture_from_it_leads_back() {
    let mut panel = SidePanel::made(MadeBody::listed(key("nas"), files(3)));
    assert_eq!(panel.host, "nas");
    assert_eq!(panel.path, "", "nothing to open in an app");
    let view = panel.view_at(2, 60, AREA, 1_760_000_000);
    assert_eq!(view.title, "Images · work · 3");
    assert_eq!(
        view.look,
        SideLook {
            refresh: true,
            back: false
        }
    );
    let SideBody::Made { rows, message } = &view.body else {
        panic!("{view:?}");
    };
    assert_eq!((rows.len(), *message), (2, None));
    assert!(panel.takes_keys());
    assert!(panel.made_body().is_some() && panel.image_body().is_none());
    // The wheel moves the list; a click on it is the app's to place.
    assert!(panel.scroll(1, 2));
    assert!(!panel.pan((0.0, 10.0), AREA));
    assert!(!panel.click());
    panel.reveal(2);

    // A picture opened from it keeps the list to go back to, as it was.
    let list = panel.made_body().unwrap().clone();
    let mut picture = SidePanel::image("nas", "/w/plots/1.png", (40, 30), (40, 30), LOCAL);
    assert!(
        !picture.takes_keys(),
        "a picture from the terminal has no keys"
    );
    assert_eq!(picture.view(20, 60, AREA).look, SideLook::default());
    picture.back = Some(list.clone());
    assert!(picture.takes_keys());
    assert_eq!(
        picture.view(20, 60, AREA).look,
        SideLook {
            refresh: false,
            back: true
        }
    );
    assert_eq!(picture.back.take(), Some(list));
    // Text has neither.
    let text = SidePanel::text(
        &Document {
            host: LOCAL.to_string(),
            path: "/w/a.rs".to_string(),
            line: None,
            text: "x".to_string(),
        },
        LOCAL,
    );
    assert!(!text.takes_keys() && text.made_body().is_none());
    assert_eq!(text.view(20, 60, AREA).look, SideLook::default());
}
