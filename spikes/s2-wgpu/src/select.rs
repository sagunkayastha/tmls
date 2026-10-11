//! Mouse selection in cell coordinates of the viewport, and the text it copies.
//!
//! A stream selection (as in a text editor): from the anchor cell to the head cell in reading
//! order, both included. Wide characters are never cut in half. Copied rows are joined with a
//! newline and lose their trailing spaces. (`Frame` does not say which rows are soft-wrapped,
//! so a wrapped line is copied as two lines.)
//!
//! Pixel → cell is `gfx::grid::Scene::cell_at` (a press) and `Scene::nearest_cell` (a drag).

use s2_core::{Frame, Wide};

use crate::gfx::grid::CellSpan;

/// `(col, row)`.
pub type CellPos = (u16, u16);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    anchor: CellPos,
    head: CellPos,
    /// False until the pointer has left the anchor cell: a click selects nothing.
    active: bool,
}

impl Selection {
    /// The button went down on `at`.
    pub fn start(at: CellPos) -> Selection {
        Selection {
            anchor: at,
            head: at,
            active: false,
        }
    }

    /// The pointer moved to `at` with the button held. Returns whether the selection changed.
    pub fn drag_to(&mut self, at: CellPos) -> bool {
        if at == self.head {
            return false;
        }
        self.head = at;
        self.active = true;
        true
    }

    pub fn is_empty(&self) -> bool {
        !self.active
    }

    /// First and last row, or `None` for an empty selection.
    pub fn rows(&self) -> Option<(u16, u16)> {
        let (start, end) = self.ordered()?;
        Some((start.1, end.1))
    }

    /// Start and end in reading order.
    fn ordered(&self) -> Option<(CellPos, CellPos)> {
        if !self.active {
            return None;
        }
        let key = |p: CellPos| (p.1, p.0);
        Some(if key(self.anchor) <= key(self.head) {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        })
    }

    /// The selected columns of each row of `frame`, widened to whole wide characters and
    /// clipped to the frame. This is what is highlighted and what `text` copies.
    pub fn spans(&self, frame: &Frame) -> Vec<CellSpan> {
        let Some((start, end)) = self.ordered() else {
            return Vec::new();
        };
        let (cols, rows) = (frame.cols as usize, frame.rows as usize);
        let wide = |row: usize, col: usize| frame.cells.get(row * cols + col).map(|cell| cell.wide);
        let mut spans = Vec::new();
        for row in start.1 as usize..=(end.1 as usize).min(rows.saturating_sub(1)) {
            if row >= rows {
                break;
            }
            let mut first = if row == start.1 as usize {
                start.0 as usize
            } else {
                0
            };
            // One past the last selected column.
            let mut last = if row == end.1 as usize {
                (end.0 as usize + 1).min(cols)
            } else {
                cols
            };
            if first >= last {
                continue;
            }
            if first > 0 && wide(row, first) == Some(Wide::Spacer) {
                first -= 1;
            }
            if last < cols
                && wide(row, last - 1) == Some(Wide::Lead)
                && wide(row, last) == Some(Wide::Spacer)
            {
                last += 1;
            }
            spans.push(CellSpan {
                row: row as u16,
                col_start: first as u16,
                col_end: last as u16,
            });
        }
        spans
    }

    /// The selected text: one line per row, trailing spaces trimmed, joined with `\n`.
    pub fn text(&self, frame: &Frame) -> String {
        let cols = frame.cols as usize;
        let mut lines = Vec::new();
        let mut next_row = None;
        for span in self.spans(frame) {
            // A row with nothing selectable (start past its end) still is a line.
            while next_row.is_some_and(|row| row < span.row) {
                lines.push(String::new());
                next_row = next_row.map(|row| row + 1);
            }
            let start = span.row as usize * cols;
            let cells = frame
                .cells
                .get(start + span.col_start as usize..start + span.col_end as usize)
                .unwrap_or(&[]);
            let mut line = String::new();
            for cell in cells {
                match (cell.wide, cell.text.is_empty()) {
                    (Wide::Spacer, _) => {}
                    (_, true) => line.push(' '),
                    (_, false) => line.push_str(&cell.text),
                }
            }
            lines.push(line.trim_end_matches(' ').to_string());
            next_row = Some(span.row + 1);
        }
        let text = lines.join("\n");
        if text.chars().all(|c| c == '\n') {
            return String::new();
        }
        text
    }
}
