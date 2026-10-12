//! Links in the terminal: finding the URL under a cell and opening it. `xdg-open` is never
//! run here: the spawner is injected.

use desktop::links;
use s2_core::{Cell, Frame, Wide};

#[test]
fn at_finds_the_url_under_the_column() {
    let row = "see https://example.com/a?b=1#c, then http://x.org.";
    let first = row.find("https").unwrap();
    assert_eq!(
        links::at(row, first).as_deref(),
        Some("https://example.com/a?b=1#c")
    );
    assert_eq!(
        links::at(row, first + 26).as_deref(),
        Some("https://example.com/a?b=1#c")
    );
    // The comma after it, the words between and the full stop at the end are no link.
    assert_eq!(links::at(row, first + 27), None);
    assert_eq!(links::at(row, 0), None);
    assert_eq!(
        links::at(row, row.len() - 2).as_deref(),
        Some("http://x.org")
    );
    assert_eq!(links::at(row, row.len() - 1), None);
    assert_eq!(links::at(row, 500), None);
    assert_eq!(links::at("", 0), None);
}

#[test]
fn localhost_stays_localhost() {
    let row = "  Local:   http://localhost:5173/app  (ready)";
    let col = row.find("localhost").unwrap();
    assert_eq!(
        links::at(row, col).as_deref(),
        Some("http://localhost:5173/app")
    );
    assert_eq!(
        links::at("http://127.0.0.1:8000/", 3).as_deref(),
        Some("http://127.0.0.1:8000/")
    );
}

#[test]
fn brackets_quotes_and_other_schemes() {
    // A closing bracket belongs to the URL only when its opening one does.
    let wiki = "(https://en.wikipedia.org/wiki/Rust_(language))";
    assert_eq!(
        links::at(wiki, 5).as_deref(),
        Some("https://en.wikipedia.org/wiki/Rust_(language)")
    );
    assert_eq!(
        links::at("<https://a.io/x>", 3).as_deref(),
        Some("https://a.io/x")
    );
    assert_eq!(
        links::at("\"https://a.io/x\"", 3).as_deref(),
        Some("https://a.io/x")
    );
    // Columns count characters, not bytes.
    assert_eq!(
        links::at("é→ https://a.io/x", 5).as_deref(),
        Some("https://a.io/x")
    );
    // The scheme alone, and schemes this app does not open, are no links.
    assert_eq!(links::at("https:// nothing", 2), None);
    assert_eq!(links::at("file:///etc/passwd ftp://a.io ssh://b", 3), None);
    assert_eq!(links::at("javascript:alert(1)", 3), None);
}

fn frame_of(row: &str) -> Frame {
    let cells: Vec<Cell> = row
        .chars()
        .map(|c| Cell {
            text: c.to_string(),
            ..Cell::default()
        })
        .collect();
    Frame {
        cols: cells.len() as u16,
        rows: 1,
        dirty_rows: vec![true],
        cells,
        cursor: None,
        seq: 1,
        t_output_ns: 0,
        keys: Vec::new(),
    }
}

#[test]
fn in_frame_reads_the_row_and_prefers_the_cells_own_hyperlink() {
    let mut frame = frame_of("go https://a.io/x now");
    assert_eq!(
        links::in_frame(&frame, (4, 0)).as_deref(),
        Some("https://a.io/x")
    );
    assert_eq!(links::in_frame(&frame, (0, 0)), None);
    assert_eq!(
        links::in_frame(&frame, (4, 7)),
        None,
        "a row that is not there"
    );
    // An OSC 8 hyperlink on the cell wins over the text.
    frame.cells[0].hyperlink = Some("https://docs.rs/".to_string());
    assert_eq!(
        links::in_frame(&frame, (0, 0)).as_deref(),
        Some("https://docs.rs/")
    );

    // A wide character takes two cells; the URL after it is still found under its cells.
    let mut wide = frame_of("你_ https://a.io/x");
    wide.cells[0].wide = Wide::Lead;
    wide.cells[1] = Cell {
        wide: Wide::Spacer,
        ..Cell::default()
    };
    assert_eq!(
        links::in_frame(&wide, (3, 0)).as_deref(),
        Some("https://a.io/x")
    );
    assert_eq!(links::in_frame(&wide, (1, 0)), None);
}

#[test]
fn open_runs_xdg_open_with_the_url_as_one_argument() {
    let mut ran: Vec<(String, Vec<String>)> = Vec::new();
    let mut spawn = |program: &str, args: &[&str]| {
        ran.push((
            program.to_string(),
            args.iter().map(|a| a.to_string()).collect(),
        ));
        Ok(())
    };
    // Shell characters in a URL are just characters of the one argument.
    let url = "https://a.io/x?q=$(reboot)&b=;rm+-rf|`id`";
    assert_eq!(links::open_with(url, &mut spawn), Ok(()));
    assert_eq!(ran, [("xdg-open".to_string(), vec![url.to_string()])]);
}

#[test]
fn open_refuses_everything_but_http_and_https() {
    let mut runs = 0;
    let mut spawn = |_: &str, _: &[&str]| {
        runs += 1;
        Ok(())
    };
    for url in [
        "file:///etc/passwd",
        "javascript:alert(1)",
        "-https://a.io",
        "--help",
        "ssh://host",
        "https://",
        "https://a.io/x y",
        "https://a.io/\nx",
        "",
    ] {
        assert!(
            links::open_with(url, &mut spawn).is_err(),
            "{url:?} must not be opened"
        );
    }
    assert_eq!(runs, 0, "nothing was run");
}

#[test]
fn a_failed_spawn_is_reported() {
    let mut spawn = |_: &str, _: &[&str]| Err(std::io::Error::other("no such file"));
    let said = links::open_with("https://a.io/", &mut spawn);
    assert_eq!(said, Err("xdg-open failed: no such file".to_string()));
}

// --- file paths -------------------------------------------------------------------------------

use desktop::links::{FileLink, Link};

fn file(path: &str, line: Option<usize>) -> Option<Link> {
    Some(Link::File(FileLink {
        path: path.to_string(),
        line,
    }))
}

/// The link under the first character of `under` in `row`.
fn on(row: &str, under: &str) -> Option<Link> {
    let byte = row
        .find(under)
        .unwrap_or_else(|| panic!("{under:?} is not in {row:?}"));
    links::link_at(row, row[..byte].chars().count())
}

#[test]
fn a_path_with_a_line_is_a_link_from_its_first_character_to_the_column() {
    let row = "error at src/main.rs:12:5: oops";
    let first = row.find("src").unwrap();
    for col in first..first + "src/main.rs:12:5".len() {
        assert_eq!(
            links::link_at(row, col),
            file("src/main.rs", Some(12)),
            "col {col}"
        );
    }
    // The colon after the column, the words around: no link.
    assert_eq!(links::link_at(row, first + "src/main.rs:12:5".len()), None);
    assert_eq!(on(row, "error"), None);
    assert_eq!(on(row, "at "), None);
    assert_eq!(on(row, "oops"), None);
    assert_eq!(links::link_at(row, 500), None);

    // tmls's FILE_LINE: a name with an extension, or anything with a slash.
    assert_eq!(on("main.rs:7", "main"), file("main.rs", Some(7)));
    assert_eq!(on("  /etc/hosts:3", "/etc"), file("/etc/hosts", Some(3)));
    assert_eq!(
        on("~/notes/a.md:10", "notes"),
        file("~/notes/a.md", Some(10))
    );
    assert_eq!(
        on("../up/Makefile:2", "up"),
        file("../up/Makefile", Some(2))
    );
    assert_eq!(
        on("a_b-c+d.tar.gz:1", "a_b"),
        file("a_b-c+d.tar.gz", Some(1))
    );
    assert_eq!(on("bin/tool:44", "tool"), file("bin/tool", Some(44)));
    assert_eq!(on("v1.2:3", "v1"), file("v1.2", Some(3)));
    // What comes before the path's characters is not the path's.
    assert_eq!(on("x=foo/bar.rs:3", "foo"), file("foo/bar.rs", Some(3)));
    assert_eq!(on("(lib.rs:9)", "lib"), file("lib.rs", Some(9)));
    assert_eq!(on("\"a b/c.py:5\"", "b/c"), file("b/c.py", Some(5)));
    // Columns count characters, not bytes, and a wide character's second cell is a space.
    assert_eq!(on("日 本 é→ a.rs:4", "a.rs"), file("a.rs", Some(4)));
    // More digits than a number holds: still a link, to the last line there is.
    assert_eq!(
        on("a.rs:99999999999999999999999999", "a.rs"),
        file("a.rs", Some(usize::MAX))
    );
}

#[test]
fn what_only_looks_like_a_path_with_a_line_is_none() {
    // A clock, a ratio, a word and a number: no extension and no slash.
    for row in ["12:30:45", "at 3:15", "note:12", "Makefile:3", "x:1"] {
        for col in 0..row.len() {
            assert_eq!(links::link_at(row, col), None, "{row:?} col {col}");
        }
    }
    // A name needs something before the dot and letters or digits after it.
    assert_eq!(on(".rs:1", ".rs"), None);
    assert_eq!(on("a.:1", "a."), None);
    // A slash at the very end is no path with a line.
    assert_eq!(on("dir/:1", "dir"), None);
    // Without digits after the colon the line is none; the name is still a path.
    assert_eq!(on("main.rs: error", "main"), file("main.rs", None));
    assert_eq!(on("main.rs:x", "main"), file("main.rs", None));
}

#[test]
fn a_url_wins_over_the_path_in_it() {
    let row = "at https://example.com/src/main.rs:12 and src/lib.rs:3";
    let url = Some(Link::Url("https://example.com/src/main.rs:12".to_string()));
    assert_eq!(on(row, "https"), url);
    assert_eq!(on(row, "example"), url);
    assert_eq!(on(row, "main.rs"), url);
    assert_eq!(on(row, "src/lib"), file("src/lib.rs", Some(3)));
    // The punctuation after a URL is neither the URL nor a path.
    let row = "see http://a.io/x/y.rs:1, ok";
    assert_eq!(on(row, ", ok"), None);
    assert_eq!(
        on(row, "y.rs"),
        Some(Link::Url("http://a.io/x/y.rs:1".to_string()))
    );
    // `at` still finds URLs only.
    assert_eq!(links::at("src/lib.rs:3", 2), None);
}

#[test]
fn an_absolute_or_home_path_without_a_line_is_a_link() {
    assert_eq!(on("wrote /data/x.png", "/data"), file("/data/x.png", None));
    assert_eq!(on("wrote /data/x.png", "x.png"), file("/data/x.png", None));
    assert_eq!(on("in ~/notes/a.md now", "~/"), file("~/notes/a.md", None));
    assert_eq!(
        on("cd /usr/local/bin", "local"),
        file("/usr/local/bin", None)
    );
    // tmls: the full stop of the sentence is not the path's, nor the bracket around it.
    let row = "see /data/x.png.";
    assert_eq!(on(row, "/data"), file("/data/x.png", None));
    assert_eq!(links::link_at(row, row.len() - 1), None);
    assert_eq!(on("(/usr/bin)", "/usr"), file("/usr/bin", None));
    assert_eq!(on("'/usr/bin'", "/usr"), file("/usr/bin", None));
    assert_eq!(on("/etc/", "/etc"), file("/etc/", None));
    // A lone slash or tilde is nothing.
    for row in ["/", "a / b", "~", "~ /", "x ~/ y"] {
        for col in 0..row.len() {
            assert_eq!(links::link_at(row, col), None, "{row:?} col {col}");
        }
    }
    // With a line it is the first kind, line and all.
    assert_eq!(on("/data/x.py:8:", "/data"), file("/data/x.py", Some(8)));
}

#[test]
fn relative_paths_and_names_with_an_extension_are_links_too() {
    // tmls's quick-select PATH.
    assert_eq!(
        on("modified: src/main.rs", "src"),
        file("src/main.rs", None)
    );
    assert_eq!(
        on("modified: src/main.rs", "main"),
        file("src/main.rs", None)
    );
    assert_eq!(on("run ./build.sh now", "./"), file("./build.sh", None));
    assert_eq!(on("in ../up/x", "../"), file("../up/x", None));
    assert_eq!(on("edit Cargo.toml", "Cargo"), file("Cargo.toml", None));
    assert_eq!(on("./.gitignore", "./"), file("./.gitignore", None));
    assert_eq!(on("a/b/c", "b"), file("a/b/c", None));
    // Unlike tmls: every dot of the name, and not the sentence's full stop.
    assert_eq!(on("got x.tar.gz", "x.tar"), file("x.tar.gz", None));
    assert_eq!(on("see src/main.rs.", "src"), file("src/main.rs", None));
    assert_eq!(on("see notes.txt.", "notes"), file("notes.txt", None));
    assert_eq!(on("see ./a/b.", "./a"), file("./a/b", None));
    // Plain words, numbers and a name that is only an extension are no paths.
    for row in [
        "hello world",
        "12 + 34",
        ".gitignore",
        "a. b",
        "end.",
        "..",
        "x ... y",
    ] {
        for col in 0..row.len() {
            assert_eq!(links::link_at(row, col), None, "{row:?} col {col}");
        }
    }
    // Not from the middle of a word in any script, nor after a dot or a slash.
    assert_eq!(links::link_at("naïve/path", 7), None);
    // The path with a line comes first where both could be.
    let row = "src/a.rs:3 src/b.rs";
    assert_eq!(on(row, "src/a"), file("src/a.rs", Some(3)));
    assert_eq!(on(row, "src/b"), file("src/b.rs", None));
}

#[test]
fn link_in_frame_reads_paths_and_prefers_the_cells_own_hyperlink() {
    let frame = frame_of("at src/main.rs:12 ok");
    assert_eq!(
        links::link_in_frame(&frame, (4, 0)),
        file("src/main.rs", Some(12))
    );
    assert_eq!(links::link_in_frame(&frame, (0, 0)), None);
    assert_eq!(links::link_in_frame(&frame, (4, 3)), None, "no such row");
    // `in_frame` is for URLs.
    assert_eq!(links::in_frame(&frame, (4, 0)), None);
    let mut marked = frame_of("src/main.rs:12");
    marked.cells[2].hyperlink = Some("https://b.io/".to_string());
    assert_eq!(
        links::link_in_frame(&marked, (2, 0)),
        Some(Link::Url("https://b.io/".to_string()))
    );
    assert_eq!(
        links::link_in_frame(&marked, (3, 0)),
        file("src/main.rs", Some(12))
    );
}

#[test]
fn open_path_runs_xdg_open_with_an_absolute_path_as_one_argument() {
    let mut ran: Vec<(String, Vec<String>)> = Vec::new();
    let mut spawn = |program: &str, args: &[&str]| {
        ran.push((
            program.to_string(),
            args.iter().map(|a| a.to_string()).collect(),
        ));
        Ok(())
    };
    let path = "/home/u/a b/it's $HOME;x.png";
    assert_eq!(links::open_path_with(path, &mut spawn), Ok(()));
    // Not a path from the root: an option, an address, a relative name.
    for bad in [
        "",
        "-h",
        "--help",
        "x.png",
        "~/x.png",
        "http://a.io/",
        "file:///etc/passwd",
        "/a\0b",
    ] {
        assert!(
            links::open_path_with(bad, &mut spawn).is_err(),
            "{bad:?} must be refused"
        );
    }
    assert_eq!(
        ran,
        vec![("xdg-open".to_string(), vec![path.to_string()])],
        "one run, the path as one argument"
    );
    let mut fails = |_: &str, _: &[&str]| Err(std::io::Error::from(std::io::ErrorKind::NotFound));
    let said = links::open_path_with("/x", &mut fails);
    assert!(said.unwrap_err().starts_with("xdg-open failed"));
}

#[test]
fn a_folder_is_opened_where_it_is_or_by_its_sftp_address() {
    let mut ran: Vec<String> = Vec::new();
    let mut spawn = |program: &str, args: &[&str]| {
        assert_eq!((program, args.len()), ("xdg-open", 1));
        ran.push(args[0].to_string());
        Ok(())
    };
    for good in ["/home/u/a b", "sftp://archbox/home/u/a%20b", "sftp://nas"] {
        assert_eq!(links::open_folder_with(good, &mut spawn), Ok(()), "{good}");
    }
    for bad in [
        "",
        "-h",
        "x",
        "sftp://",
        "sftp:///etc",
        "sftp://-oProxyCommand=x/",
        "sftp://h/a b",
        "sftp://h/a\nb",
        "http://a.io/",
        "file:///etc",
    ] {
        assert!(
            links::open_folder_with(bad, &mut spawn).is_err(),
            "{bad:?} must be refused"
        );
    }
    assert_eq!(
        ran,
        ["/home/u/a b", "sftp://archbox/home/u/a%20b", "sftp://nas"]
    );
}
