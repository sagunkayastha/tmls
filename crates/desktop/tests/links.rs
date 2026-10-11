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
