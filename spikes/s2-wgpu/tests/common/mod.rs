//! Frame builders shared by the integration tests.
#![allow(dead_code)]

use s2_core::{Cell, Cursor, CursorShape, Frame, Wide};

pub fn cell(text: &str) -> Cell {
    Cell {
        text: text.to_string(),
        ..Cell::default()
    }
}

pub fn lead(text: &str) -> Cell {
    Cell {
        text: text.to_string(),
        wide: Wide::Lead,
        ..Cell::default()
    }
}

pub fn spacer() -> Cell {
    Cell {
        wide: Wide::Spacer,
        ..Cell::default()
    }
}

/// A frame from rows of cells (all rows the same length), every row dirty, no cursor.
pub fn frame(rows: Vec<Vec<Cell>>) -> Frame {
    let cols = rows[0].len();
    assert!(rows.iter().all(|r| r.len() == cols), "ragged test frame");
    Frame {
        cols: cols as u16,
        rows: rows.len() as u16,
        dirty_rows: vec![true; rows.len()],
        cells: rows.into_iter().flatten().collect(),
        cursor: None,
        seq: 1,
        t_output_ns: 0,
        keys: Vec::new(),
    }
}

/// One narrow cell per char, rows padded with blanks to `cols`.
pub fn text_frame(lines: &[&str], cols: usize) -> Frame {
    let rows = lines
        .iter()
        .map(|line| {
            let mut row: Vec<Cell> = line
                .chars()
                .map(|c| {
                    if c == ' ' {
                        Cell::default()
                    } else {
                        cell(&c.to_string())
                    }
                })
                .collect();
            row.resize(cols, Cell::default());
            row
        })
        .collect();
    frame(rows)
}

pub fn cursor(col: u16, row: u16, shape: CursorShape) -> Option<Cursor> {
    Some(Cursor {
        col,
        row,
        shape,
        visible: true,
    })
}
