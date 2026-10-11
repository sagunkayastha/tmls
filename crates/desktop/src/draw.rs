//! Where everything in the window is, and everything but the terminal's cells as quads.
//!
//! ```text
//! +-- sidebar (28 cols) --+s+-- tab bar (1 row) ---------------------+
//! | session row           |e| ▌● name  × | ▌? name  × |              |
//! | session row           |p+----------------------------------------+
//! | …                     |a| terminal: the grid of the tab in front |
//! |                       |r|                                        |
//! |                       |a+----------------------------------------+
//! |                       |t| status line (1 row)                    |
//! +-----------------------+o+----------------------------------------+
//! ```
//!
//! `layout` gives the rectangles in physical pixels, all on whole pixels (atlas texels are
//! copied 1:1), `Layout::hit` what is under a pixel. `Layout::terminal` is the origin of the
//! tabs' scenes and `Layout::grid` the size the sessions are resized to.
//!
//! The quads are two layers of `gfx::grid::Instance`s around the terminal's own:
//! `[chrome.base, scene.bg, scene.glyphs, scene.overlay, chrome.top]` (`layer_order`) — `base`
//! is the sidebar, the tab bar and the status line, `top` the notice over the terminal of a tab
//! whose session ended. `Chrome` caches both: they are rebuilt when `invalidate` was called or
//! the atlas started a new generation, not on every frame, so a key echo pays nothing for them.
//!
//! Text is set on the atlas's cell grid and cut to the columns that fit (the renderer has no
//! scissor). The sidebar here is a plain list of sessions; Task 4 replaces its model and
//! drawing.

use s2_wgpu::gfx::atlas::{Atlas, CellMetrics, FontStyle};
use s2_wgpu::gfx::grid::{self, Instance, Scene, push_text, solid};
use s2_wgpu::ui::draw::fit;
pub use s2_wgpu::ui::layout::Rect;

use crate::tabs::Front;
use crate::theme::{CLOSE_MARK, SIDEBAR_COLS, TAB_MAX_COLS, Theme};

/// What a tab whose session ended shows over its last screen.
pub const ENDED_NOTICE: &str = "session ended — Enter to reattach";

/// Cells of a tab that are not its name: the host bar's cell, the mark, the `×` and the spaces
/// between them — `▌● name × `.
const TAB_FIXED_COLS: usize = 6;
/// A tab narrower than this has no `×` (and no place to click to close it).
const TAB_CLOSE_MIN_COLS: usize = 4;
/// Cells at a tab's right end that close it when clicked: the `×` and one cell either side.
const TAB_CLOSE_COLS: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub window: (f32, f32),
    /// Cell width and height the layout was made for.
    pub cell: (f32, f32),
    pub sidebar: Rect,
    /// The line between the sidebar and the rest.
    pub separator: Rect,
    /// Height of a row of the session list; `visible_rows` fit.
    pub row_h: f32,
    pub visible_rows: usize,
    /// Columns a row's label may take.
    pub label_cols: usize,
    pub tab_bar: Rect,
    /// The grid's area; its corner is the grid's origin.
    pub terminal: Rect,
    pub status: Rect,
    /// Columns and rows of the terminal.
    pub grid: (u16, u16),
    /// Width of the host colour bar at a tab's left edge.
    pub host_bar: f32,
}

/// What is under a window pixel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// A row of the session list, counted from the first visible one (it may be empty).
    ListRow(usize),
    /// The sidebar or the separator, where no row is.
    Sidebar,
    /// A tab, anywhere but its `×`.
    Tab(usize),
    /// The `×` of a tab.
    TabClose(usize),
    /// The tab bar right of the last tab.
    TabBar,
    Terminal,
    Status,
    Outside,
}

/// Lays the window out. `window` is the surface size in physical pixels, `m` the cell box of
/// the atlas at the current scale and font size.
pub fn layout(window: (u32, u32), scale: f32, m: &CellMetrics, theme: &Theme) -> Layout {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let px = |logical: f32| (logical * scale).round().max(0.0);
    let (win_w, win_h) = (window.0 as f32, window.1 as f32);
    let (cell_w, cell_h) = (m.width.max(1.0), m.height.max(1.0));
    let inset = px(theme.inset);
    let line = px(theme.border).max(1.0);

    // The sidebar never takes more than half the window.
    let side_w = (SIDEBAR_COLS as f32 * cell_w).min((win_w / 2.0).floor());
    let sep_w = line.min((win_w - side_w).max(0.0));
    let right_x = side_w + sep_w;
    let right_w = (win_w - right_x).max(0.0);

    // The tab bar and the status line never take more than a third of the height each.
    let bar_h = (cell_h + 2.0 * inset).min((win_h / 3.0).floor());
    let terminal = Rect {
        x: right_x,
        y: bar_h,
        w: right_w,
        h: (win_h - 2.0 * bar_h).max(0.0),
    };
    let row_h = cell_h + inset;
    Layout {
        window: (win_w, win_h),
        cell: (cell_w, cell_h),
        sidebar: Rect {
            x: 0.0,
            y: 0.0,
            w: side_w,
            h: win_h,
        },
        separator: Rect {
            x: side_w,
            y: 0.0,
            w: sep_w,
            h: win_h,
        },
        row_h,
        visible_rows: ((win_h - inset).max(0.0) / row_h).floor() as usize,
        label_cols: ((side_w / cell_w).floor() as usize).saturating_sub(2),
        tab_bar: Rect {
            x: right_x,
            y: 0.0,
            w: right_w,
            h: bar_h,
        },
        terminal,
        status: Rect {
            x: right_x,
            y: terminal.bottom(),
            w: right_w,
            h: bar_h,
        },
        grid: m.grid_for(terminal.w as u32, terminal.h as u32),
        host_bar: px(theme.host_bar).clamp(1.0, cell_w),
    }
}

/// The first row of the session list that is drawn, so that the selected one is in view.
pub fn list_top(selected: usize, visible_rows: usize) -> usize {
    (selected + 1).saturating_sub(visible_rows)
}

impl Layout {
    /// Width of a tab in cells when `tabs` are open: they share the bar evenly, up to
    /// `TAB_MAX_COLS` each.
    pub fn tab_cols(&self, tabs: usize) -> usize {
        let bar_cols = (self.tab_bar.w / self.cell.0).floor() as usize;
        (bar_cols / tabs.max(1)).min(TAB_MAX_COLS)
    }

    /// The rectangle of tab `index` of `tabs`.
    pub fn tab_rect(&self, index: usize, tabs: usize) -> Rect {
        let w = self.tab_cols(tabs) as f32 * self.cell.0;
        Rect {
            x: self.tab_bar.x + index as f32 * w,
            w,
            ..self.tab_bar
        }
    }

    /// The rectangle of a visible row of the session list.
    pub fn list_row(&self, row: usize) -> Rect {
        let top = self.row_h - self.cell.1;
        Rect {
            x: 0.0,
            y: top + row as f32 * self.row_h,
            w: self.sidebar.w,
            h: self.row_h,
        }
    }

    /// What is under a pixel while `tabs` tabs are open.
    pub fn hit(&self, x: f32, y: f32, tabs: usize) -> Hit {
        let window = Rect {
            x: 0.0,
            y: 0.0,
            w: self.window.0,
            h: self.window.1,
        };
        if !window.contains(x, y) {
            return Hit::Outside;
        }
        if x < self.tab_bar.x {
            let row = ((y - self.list_row(0).y) / self.row_h).floor();
            let in_list =
                self.sidebar.contains(x, y) && row >= 0.0 && (row as usize) < self.visible_rows;
            return if in_list {
                Hit::ListRow(row as usize)
            } else {
                Hit::Sidebar
            };
        }
        if self.terminal.contains(x, y) {
            return Hit::Terminal;
        }
        if !self.tab_bar.contains(x, y) {
            return Hit::Status;
        }
        let cols = self.tab_cols(tabs);
        let col = ((x - self.tab_bar.x) / self.cell.0).floor() as usize;
        if cols == 0 || col / cols >= tabs {
            return Hit::TabBar;
        }
        let index = col / cols;
        if cols >= TAB_CLOSE_MIN_COLS && col % cols >= cols - TAB_CLOSE_COLS {
            Hit::TabClose(index)
        } else {
            Hit::Tab(index)
        }
    }
}

/// One tab of the tab bar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabView {
    pub name: String,
    /// The session's status mark and its colour; `None` while the listing does not know the
    /// session.
    pub mark: Option<(char, [u8; 3])>,
    /// The host's accent colour.
    pub host: [u8; 3],
    /// The session ended: the name is dimmed.
    pub ended: bool,
}

/// Everything `Chrome` draws, as the app last put it together.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct View {
    /// The session list's rows, all of them.
    pub rows: Vec<String>,
    pub selected: usize,
    pub sidebar_focused: bool,
    pub tabs: Vec<TabView>,
    pub active: Option<usize>,
    pub status: String,
    /// Shown centred over the terminal (`ENDED_NOTICE`, or that no session is open).
    pub notice: Option<String>,
}

fn opaque(c: [u8; 3]) -> [u8; 4] {
    [c[0], c[1], c[2], 255]
}

/// Collects quads: rectangles and text on the cell grid.
struct Painter<'a> {
    out: &'a mut Vec<Instance>,
    atlas: &'a mut Atlas,
    m: CellMetrics,
}

impl Painter<'_> {
    fn rect(&mut self, r: Rect, colour: [u8; 3]) {
        if r.w > 0.0 && r.h > 0.0 {
            self.out.push(solid([r.x, r.y], [r.w, r.h], opaque(colour)));
        }
    }

    /// `text` from `(x, y)` (top-left of its first cell), cut to `cols` columns.
    fn label(&mut self, text: &str, x: f32, y: f32, cols: usize, colour: [u8; 3]) {
        for (cluster, col) in fit(text, cols) {
            let x = x + col as f32 * self.m.width;
            push_text(
                self.out,
                self.atlas,
                &cluster,
                FontStyle::default(),
                x,
                y,
                &self.m,
                opaque(colour),
            );
        }
    }

    /// The y that centres one line of text in `r`.
    fn text_y(&self, r: Rect) -> f32 {
        r.y + ((r.h - self.m.height) / 2.0).floor().max(0.0)
    }
}

/// The cached quads of everything but the terminal.
#[derive(Default)]
pub struct Chrome {
    base: Vec<Instance>,
    top: Vec<Instance>,
    /// Atlas generation the quads were built in.
    built: Option<u64>,
    dirty: bool,
}

impl Chrome {
    pub fn new() -> Chrome {
        Chrome::default()
    }

    /// The view or the layout changed: rebuild at the next `update`.
    pub fn invalidate(&mut self) {
        self.dirty = true;
    }

    /// Sidebar, tab bar and status line: drawn before the terminal.
    pub fn base(&self) -> &[Instance] {
        &self.base
    }

    /// The notice over the terminal: drawn after it.
    pub fn top(&self) -> &[Instance] {
        &self.top
    }

    /// Rebuilds the quads if needed; returns whether it did.
    pub fn update(
        &mut self,
        view: &View,
        layout: &Layout,
        theme: &Theme,
        atlas: &mut Atlas,
    ) -> bool {
        if !self.dirty && self.built == Some(atlas.generation()) {
            return false;
        }
        // A second pass only if the atlas filled up and restarted during the first: quads
        // built before the restart then point at glyphs that are gone.
        for _pass in 0..2 {
            let generation = atlas.generation();
            self.base.clear();
            self.top.clear();
            let m = atlas.metrics();
            let mut p = Painter {
                out: &mut self.base,
                atlas,
                m,
            };
            sidebar(&mut p, view, layout, theme);
            tab_bar(&mut p, view, layout, theme);
            status_line(&mut p, view, layout, theme);
            p.out = &mut self.top;
            notice(&mut p, view, layout, theme);
            if atlas.generation() == generation {
                break;
            }
        }
        self.built = Some(atlas.generation());
        self.dirty = false;
        true
    }
}

fn sidebar(p: &mut Painter, view: &View, l: &Layout, theme: &Theme) {
    p.rect(l.sidebar, theme.sidebar_bg);
    // The line beside the sidebar shows which side has the keyboard focus.
    let beside = if view.sidebar_focused {
        theme.focus_ring
    } else {
        theme.separator
    };
    p.rect(l.separator, beside);
    let top = list_top(view.selected, l.visible_rows);
    for row in 0..l.visible_rows {
        let Some(label) = view.rows.get(top + row) else {
            break;
        };
        let r = l.list_row(row);
        let mut colour = theme.sidebar_fg;
        if top + row == view.selected {
            p.rect(r, theme.row_selected_bg);
            colour = theme.row_selected_fg;
            if view.sidebar_focused {
                p.rect(
                    Rect {
                        w: (p.m.width / 4.0).round().max(1.0),
                        ..r
                    },
                    theme.focus_ring,
                );
            }
        }
        let y = p.text_y(r);
        p.label(label, r.x + p.m.width, y, l.label_cols, colour);
    }
}

fn tab_bar(p: &mut Painter, view: &View, l: &Layout, theme: &Theme) {
    p.rect(l.tab_bar, theme.bar_bg);
    let tabs = view.tabs.len();
    let cols = l.tab_cols(tabs);
    let cell_w = p.m.width;
    let line = l.separator.w.max(1.0);
    for (index, tab) in view.tabs.iter().enumerate() {
        let r = l.tab_rect(index, tabs);
        let active = view.active == Some(index);
        if active {
            p.rect(r, theme.tab_active_bg);
        }
        p.rect(
            Rect {
                w: l.host_bar.min(r.w),
                ..r
            },
            tab.host,
        );
        p.rect(
            Rect {
                x: r.right() - line,
                w: line.min(r.w),
                ..r
            },
            theme.separator,
        );
        let y = p.text_y(r);
        let at = |col: usize| r.x + col as f32 * cell_w;
        if let Some((mark, colour)) = tab.mark.filter(|_| cols >= 3) {
            p.label(mark.encode_utf8(&mut [0; 4]), at(1), y, 1, colour);
        }
        let name_colour = match (active, tab.ended) {
            (true, false) => theme.tab_active_fg,
            (false, false) => theme.sidebar_fg,
            (_, true) => theme.muted_fg,
        };
        p.label(
            &tab.name,
            at(3),
            y,
            cols.saturating_sub(TAB_FIXED_COLS),
            name_colour,
        );
        if cols >= TAB_CLOSE_MIN_COLS {
            p.label(CLOSE_MARK, at(cols - 2), y, 1, theme.muted_fg);
        }
    }
}

fn status_line(p: &mut Painter, view: &View, l: &Layout, theme: &Theme) {
    p.rect(l.status, theme.bar_bg);
    let cols = ((l.status.w / p.m.width).floor() as usize).saturating_sub(2);
    if l.status.h >= p.m.height {
        let y = p.text_y(l.status);
        p.label(
            &view.status,
            l.status.x + p.m.width,
            y,
            cols,
            theme.muted_fg,
        );
    }
}

/// The notice, centred in the terminal's area on a band of its own colour.
fn notice(p: &mut Painter, view: &View, l: &Layout, theme: &Theme) {
    let Some(text) = &view.notice else {
        return;
    };
    let t = l.terminal;
    let cols = (t.w / p.m.width).floor() as usize;
    let used = fit(text, cols.saturating_sub(4))
        .last()
        .map_or(0, |(_, col)| col + 1);
    if used == 0 || t.h < 3.0 * p.m.height {
        return;
    }
    let w = (used + 4) as f32 * p.m.width;
    let band = Rect {
        x: t.x + ((t.w - w) / 2.0).floor().max(0.0),
        y: t.y + ((t.h - 3.0 * p.m.height) / 2.0).floor(),
        w,
        h: 3.0 * p.m.height,
    };
    p.rect(band, theme.notice_bg);
    p.label(
        text,
        band.x + 2.0 * p.m.width,
        band.y + p.m.height,
        used,
        theme.notice_fg,
    );
}

/// The draw order for `Renderer::prepare`: the chrome under the terminal's three layers, the
/// notice over them. `scene` is the scene of the tab in front, if there is one to draw.
pub fn layer_order<'a>(scene: Option<&'a Scene>, chrome: &'a Chrome) -> [&'a [Instance]; 5] {
    let [bg, glyphs, overlay] = scene.map_or([&[][..]; 3], Scene::layers);
    [chrome.base(), bg, glyphs, overlay, chrome.top()]
}

/// Everything drawn besides new cells, for one render: the overlays of the tab in front
/// (cursor, preedit, selection) and the chrome.
///
/// `refeed`: no new frame was taken for this render, so the front tab's newest frame goes
/// through `Scene::update_cells` again — that picks up a new origin, scale factor or font size.
///
/// The overlays and the chrome both draw glyphs, and either can fill the atlas and restart
/// it, which voids the other's quads and the cells'. `Scene::update_overlays` rebuilds the
/// cells when it finds them stale and `Chrome::update` rebuilds on a new generation, so the two
/// are repeated until the chrome was built in the atlas's current generation.
pub fn compose(
    front: Option<Front<'_>>,
    refeed: bool,
    atlas: &mut Atlas,
    theme: &Theme,
    chrome: &mut Chrome,
    view: &View,
    layout: &Layout,
) {
    let Some(front) = front else {
        chrome.update(view, layout, theme, atlas);
        return;
    };
    if refeed {
        front
            .scene
            .update_cells(front.frame, atlas, &theme.terminal);
    }
    let overlays: grid::Overlays = front.session.overlays();
    for _round in 0..3 {
        chrome.update(view, layout, theme, atlas);
        front
            .scene
            .update_overlays(front.frame, atlas, &theme.terminal, &overlays);
        if chrome.built == Some(atlas.generation()) {
            break;
        }
    }
}
