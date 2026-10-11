//! A one-line text field: text, caret, selection, IME preedit, and the part of it that is
//! visible in a field of a given width.
//!
//! Positions (`caret`, `selection`) are byte offsets into the text, always at the start of a
//! character: a base character and the zero-width marks after it move, select and delete as
//! one. Columns are terminal columns (`unicode-width`): a wide character takes two.
//!
//! IME: `set_preedit` holds the text being composed; it is shown at the caret but is not part
//! of `text` until `commit`. Unlike the terminal's preedit (dropped at the right edge), the
//! field scrolls so that the end of the composition is always visible (`view`).

use std::ops::Range;

use unicode_width::UnicodeWidthChar;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextField {
    text: String,
    caret: usize,
    /// The other end of the selection; nothing is selected while it equals `caret`.
    anchor: Option<usize>,
    preedit: String,
    /// First visible column (horizontal scroll), kept up to date by `view`.
    first_col: usize,
}

/// What to draw, in columns counted from the field's left edge.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FieldView {
    /// `(character, column, width)` of every character fully inside the field.
    pub cells: Vec<(String, usize, usize)>,
    /// The caret sits before this column (`cols` = at the right edge).
    pub caret_col: usize,
    /// Selected columns, end exclusive.
    pub selection: Option<(usize, usize)>,
    /// Columns of the composition (underlined), end exclusive.
    pub preedit: Option<(usize, usize)>,
}

fn width(c: char) -> usize {
    c.width().unwrap_or(0).min(2)
}

fn columns(s: &str) -> usize {
    s.chars().map(width).sum()
}

/// Line ends and tabs become one space each, other control characters go.
fn one_line(s: &str) -> String {
    s.replace("\r\n", " ")
        .chars()
        .filter_map(|c| match c {
            '\n' | '\r' | '\t' => Some(' '),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect()
}

impl TextField {
    pub fn new() -> TextField {
        TextField::default()
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Byte offset of the caret.
    pub fn caret(&self) -> usize {
        self.caret
    }

    /// The selected bytes; `None` when nothing is selected.
    pub fn selection(&self) -> Option<Range<usize>> {
        let anchor = self.anchor.filter(|&anchor| anchor != self.caret)?;
        Some(anchor.min(self.caret)..anchor.max(self.caret))
    }

    /// The text being composed (empty: none).
    pub fn preedit(&self) -> &str {
        &self.preedit
    }

    /// Start of the character before `at` (0 at the start).
    fn before(&self, at: usize) -> usize {
        self.text[..at]
            .char_indices()
            .rev()
            .find(|&(_, c)| width(c) > 0)
            .map_or(0, |(i, _)| i)
    }

    /// End of the character at `at` (the length at the end).
    fn after(&self, at: usize) -> usize {
        self.text[at..]
            .char_indices()
            .skip(1)
            .find(|&(_, c)| width(c) > 0)
            .map_or(self.text.len(), |(i, _)| at + i)
    }

    fn delete_selection(&mut self) -> bool {
        let Some(range) = self.selection() else {
            return false;
        };
        self.caret = range.start;
        self.text.replace_range(range, "");
        self.anchor = None;
        true
    }

    /// Typed text: replaces the selection, the caret ends after it. Control characters are
    /// left out.
    pub fn insert(&mut self, s: &str) {
        let clean: String = s.chars().filter(|c| !c.is_control()).collect();
        if clean.is_empty() {
            return;
        }
        self.delete_selection();
        self.anchor = None;
        self.text.insert_str(self.caret, &clean);
        self.caret += clean.len();
    }

    /// Clipboard text, made one line (`one_line`).
    pub fn paste(&mut self, s: &str) {
        self.insert(&one_line(s));
    }

    /// Deletes the selection, or else the character before the caret. Returns whether
    /// anything was deleted.
    pub fn backspace(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }
        self.anchor = None;
        let start = self.before(self.caret);
        if start == self.caret {
            return false;
        }
        self.text.replace_range(start..self.caret, "");
        self.caret = start;
        true
    }

    /// Deletes the selection, or else the character after the caret.
    pub fn delete(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }
        self.anchor = None;
        let end = self.after(self.caret);
        if end == self.caret {
            return false;
        }
        self.text.replace_range(self.caret..end, "");
        true
    }

    /// Moves the caret to `to`; with `select` the selection's other end stays where it is.
    fn move_to(&mut self, to: usize, select: bool) {
        if select {
            self.anchor.get_or_insert(self.caret);
        } else {
            self.anchor = None;
        }
        self.caret = to;
    }

    /// One character left. Without `select`, a selection collapses to its left end instead.
    pub fn left(&mut self, select: bool) {
        let to = match self.selection() {
            Some(range) if !select => range.start,
            _ => self.before(self.caret),
        };
        self.move_to(to, select);
    }

    /// One character right. Without `select`, a selection collapses to its right end instead.
    pub fn right(&mut self, select: bool) {
        let to = match self.selection() {
            Some(range) if !select => range.end,
            _ => self.after(self.caret),
        };
        self.move_to(to, select);
    }

    pub fn home(&mut self, select: bool) {
        self.move_to(0, select);
    }

    pub fn end(&mut self, select: bool) {
        self.move_to(self.text.len(), select);
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.caret = self.text.len();
    }

    pub fn selected_text(&self) -> Option<String> {
        self.selection().map(|range| self.text[range].to_string())
    }

    /// Removes and returns the selected text.
    pub fn cut(&mut self) -> Option<String> {
        let text = self.selected_text()?;
        self.delete_selection();
        Some(text)
    }

    /// `Ime::Preedit`: the text being composed (empty ends the composition without a
    /// commit). Returns whether it changed.
    pub fn set_preedit(&mut self, text: &str) -> bool {
        let clean: String = text.chars().filter(|c| !c.is_control()).collect();
        let changed = clean != self.preedit;
        self.preedit = clean;
        changed
    }

    /// `Ime::Commit`: ends the composition and inserts the text, once.
    pub fn commit(&mut self, text: &str) {
        self.preedit.clear();
        self.insert(text);
    }

    pub fn clear(&mut self) {
        *self = TextField::default();
    }

    /// The character boundary nearest to visible column `col` (the lower one on a tie).
    fn boundary_at(&self, col: usize) -> usize {
        let target = self.first_col + col;
        let mut at = 0usize;
        let mut best = (0, target);
        for (i, c) in self.text.char_indices().filter(|&(_, c)| width(c) > 0) {
            let distance = at.abs_diff(target);
            if distance < best.1 {
                best = (i, distance);
            }
            at += width(c);
        }
        if at.abs_diff(target) < best.1 {
            best.0 = self.text.len();
        }
        best.0
    }

    /// Mouse button down at visible column `col` (`Layout::hit`): the caret goes there and
    /// the selection ends.
    pub fn click(&mut self, col: usize) {
        self.caret = self.boundary_at(col);
        self.anchor = Some(self.caret);
    }

    /// Pointer moved to visible column `col` with the button held: selects from where the
    /// button went down. Returns whether the caret moved.
    pub fn drag(&mut self, col: usize) -> bool {
        let to = self.boundary_at(col);
        let moved = to != self.caret;
        self.anchor.get_or_insert(self.caret);
        self.caret = to;
        moved
    }

    /// What a field `cols` columns wide shows. Scrolls just enough to keep the caret — and
    /// with it the end of a composition — inside; `&mut` because the scroll position is kept
    /// between calls (moving the caret inside the visible part does not scroll).
    pub fn view(&mut self, cols: usize) -> FieldView {
        if cols == 0 {
            return FieldView::default();
        }
        // Absolute columns: the composition starts at the caret, the caret is drawn after it.
        let compose_start = columns(&self.text[..self.caret]);
        let compose_cols = columns(&self.preedit);
        let caret = compose_start + compose_cols;
        let total = columns(&self.text) + compose_cols;

        let mut first = self.first_col;
        if caret > first + cols {
            first = caret - cols;
        }
        if compose_start < first {
            // The whole composition if it fits, else its end.
            first = if caret - compose_start <= cols {
                compose_start
            } else {
                caret - cols
            };
        }
        first = first.min(total.saturating_sub(cols));
        self.first_col = first;

        // Clips absolute columns `start..end` to the field.
        let clip = |start: usize, end: usize| {
            let (start, end) = (start.max(first), end.min(first + cols));
            (start < end).then(|| (start - first, end - first))
        };
        let mut view = FieldView {
            cells: Vec::new(),
            caret_col: caret - first,
            selection: self.selection().and_then(|range| {
                // Text after the caret is pushed right by the composition.
                let col = |at: usize| {
                    columns(&self.text[..at]) + if at > self.caret { compose_cols } else { 0 }
                };
                clip(col(range.start), col(range.end))
            }),
            preedit: clip(compose_start, caret),
        };
        let (before, after) = self.text.split_at(self.caret);
        let mut at = 0;
        for c in before
            .chars()
            .chain(self.preedit.chars())
            .chain(after.chars())
        {
            let w = width(c);
            if w == 0 {
                // A mark joins the character before it, if that one is drawn.
                if let Some(last) = view.cells.last_mut().filter(|l| l.1 + l.2 == at - first) {
                    last.0.push(c);
                }
                continue;
            }
            // A character cut by either edge is not drawn at all.
            if at >= first && at + w <= first + cols {
                view.cells.push((c.to_string(), at - first, w));
            }
            at += w;
        }
        view
    }
}
