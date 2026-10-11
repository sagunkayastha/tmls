//! The sidebar list: items, the selected one, and the first one on screen.
//!
//! `visible` in every call is the number of rows that fit (`Layout::visible_rows`); the list
//! keeps the selection on screen when the keyboard moves it. The wheel moves only the view.

use super::focus::ListKey;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct List {
    items: Vec<String>,
    selected: usize,
    /// Index of the item in the first visible row.
    top: usize,
}

impl List {
    pub fn new(items: Vec<String>) -> List {
        List {
            items,
            selected: 0,
            top: 0,
        }
    }

    /// Static sample session names (no session is started for them).
    pub fn sample() -> List {
        const HOSTS: [&str; 4] = ["archbox", "pc", "ph", "nano"];
        const TASKS: [&str; 8] = [
            "shell", "build", "logs", "editor", "tests", "deploy", "htop", "日志",
        ];
        let items = (0..32)
            .map(|i| format!("{:02} {} · {}", i + 1, HOSTS[i % 4], TASKS[(i / 4) % 8]))
            .collect();
        List::new(items)
    }

    pub fn items(&self) -> &[String] {
        &self.items
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn top(&self) -> usize {
        self.top
    }

    fn max_top(&self, visible: usize) -> usize {
        self.items.len().saturating_sub(visible.max(1))
    }

    /// A list key. Returns whether the selection or the view changed.
    pub fn key(&mut self, key: ListKey, visible: usize) -> bool {
        let Some(last) = self.items.len().checked_sub(1) else {
            return false;
        };
        let page = visible.max(1);
        let selected = match key {
            ListKey::Up => self.selected.saturating_sub(1),
            ListKey::Down => (self.selected + 1).min(last),
            ListKey::PageUp => self.selected.saturating_sub(page),
            ListKey::PageDown => (self.selected + page).min(last),
            ListKey::Home => 0,
            ListKey::End => last,
            ListKey::Activate => return false,
        };
        let moved = selected != self.selected;
        self.selected = selected;
        self.fit(visible) || moved
    }

    /// A click on visible row `row`. Returns whether there is an item in that row (it is then
    /// the selected one).
    pub fn click_row(&mut self, row: usize) -> bool {
        let index = self.top + row;
        if index >= self.items.len() {
            return false;
        }
        self.selected = index;
        true
    }

    /// Moves the view by `rows` (positive: towards the end). Returns whether it moved.
    pub fn scroll(&mut self, rows: i32, visible: usize) -> bool {
        let top = (self.top as i64 + rows as i64).clamp(0, self.max_top(visible) as i64) as usize;
        let moved = top != self.top;
        self.top = top;
        moved
    }

    /// Appends an item, selects it and brings it into view.
    pub fn push(&mut self, name: String, visible: usize) {
        self.items.push(name);
        self.selected = self.items.len() - 1;
        self.fit(visible);
    }

    /// Brings the selected item into view and leaves no empty rows at the end. Returns
    /// whether the view moved. Call it after the number of visible rows changed.
    pub fn fit(&mut self, visible: usize) -> bool {
        let rows = visible.max(1);
        let mut top = self.top;
        if self.selected < top {
            top = self.selected;
        } else if self.selected >= top + rows {
            top = self.selected + 1 - rows;
        }
        top = top.min(self.max_top(visible));
        let moved = top != self.top;
        self.top = top;
        moved
    }
}
