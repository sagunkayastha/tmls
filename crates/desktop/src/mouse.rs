//! What the mouse does over the terminal: to the program, or a selection of our own.
//!
//! The program in the tab (tmux with `mouse on`) can ask for mouse reports. While it does
//! (`Session::mouse_reporting`):
//! - the wheel sends wheel reports, one per notch — tmux scrolls its own history; the viewport
//!   here does not move;
//! - a press, a drag and the release of the left, middle or right button go to the program.
//!
//! While it does not, the wheel scrolls the viewport and a left drag selects, as before.
//!
//! Modifiers, the same with and without reports:
//! - **Ctrl or Shift + left drag** always selects here, and the selection is copied when the
//!   button is released (so does a plain drag when the program takes no reports);
//! - **Ctrl + left click** (no movement) opens the link under the pointer;
//! - **Shift + wheel** scrolls the viewport here.
//!
//! None of these reach the program. `Pointer` decides (`Act`), `perform` does it to a
//! session and says what is left for the window to do (`Effect`). No window is needed for
//! either.

use s2_core::{Mods, MouseButton, MouseKind};
use s2_wgpu::input::WHEEL_LINES;
use s2_wgpu::select::CellPos;
use s2_wgpu::session::Session;

use crate::links;

/// No more wheel reports than this for one wheel event (a touchpad fling).
const MAX_NOTCHES: i32 = 20;

/// One thing a mouse event leads to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    /// A report for the program.
    Report {
        kind: MouseKind,
        button: Option<MouseButton>,
        at: CellPos,
        mods: Mods,
    },
    /// A selection of our own starts on this cell (`None`: the press was on no cell).
    SelectStart(Option<CellPos>),
    SelectDrag(CellPos),
    SelectEnd,
    /// The selection ends: the click is the program's.
    SelectClear,
    /// The selection goes to the clipboard.
    Copy,
    /// The link under this cell, if there is one, is opened.
    OpenLink(CellPos),
    /// The viewport moves by this many rows (negative: up, into the scrollback).
    Scroll(i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Held {
    /// The program got the press of this button and gets its drag and release.
    Program(MouseButton),
    /// The left button selects here. `ctrl`: it went down with Ctrl. `moved`: the pointer has
    /// been on another cell since.
    Select {
        from: Option<CellPos>,
        ctrl: bool,
        moved: bool,
    },
}

/// The mouse over the terminal between events: which button is held and for whom.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pointer {
    held: Option<Held>,
    /// The cell last reported to the program; motion inside it is not reported again.
    reported: Option<CellPos>,
    /// Rows the wheel has turned that did not make a whole notch yet.
    wheel_rows: i32,
}

impl Pointer {
    /// A button is held, so pointer motion matters wherever the pointer is.
    pub fn held(&self) -> bool {
        self.held.is_some()
    }

    /// The tab in front changed or was closed: a button still held is not held for the tab
    /// that is in front now, which gets neither its motion nor its release.
    pub fn reset(&mut self) {
        *self = Pointer::default();
    }

    /// A button went down over the terminal. `at`: the cell nearest to the pointer (`None`
    /// before the first frame). `reporting`: the program asked for mouse reports.
    pub fn press(
        &mut self,
        button: MouseButton,
        at: Option<CellPos>,
        mods: Mods,
        reporting: bool,
    ) -> Vec<Act> {
        if !reporting || mods.ctrl || mods.shift {
            if button != MouseButton::Left {
                return Vec::new();
            }
            self.held = Some(Held::Select {
                from: at,
                ctrl: mods.ctrl,
                moved: false,
            });
            return vec![Act::SelectStart(at)];
        }
        let Some(at) = at else {
            return Vec::new();
        };
        self.held = Some(Held::Program(button));
        self.reported = Some(at);
        let kind = MouseKind::Press;
        let button = Some(button);
        vec![
            Act::SelectClear,
            Act::Report {
                kind,
                button,
                at,
                mods,
            },
        ]
    }

    /// The pointer is over cell `at` (or nearest to it, while a button is held).
    pub fn motion(&mut self, at: CellPos, mods: Mods, reporting: bool) -> Vec<Act> {
        let button = match &mut self.held {
            Some(Held::Select { from, moved, .. }) => {
                *moved |= *from != Some(at);
                return vec![Act::SelectDrag(at)];
            }
            Some(Held::Program(button)) => Some(*button),
            None if reporting => None,
            None => {
                self.reported = None;
                return Vec::new();
            }
        };
        if self.reported == Some(at) {
            return Vec::new();
        }
        self.reported = Some(at);
        let kind = MouseKind::Motion;
        vec![Act::Report {
            kind,
            button,
            at,
            mods,
        }]
    }

    /// A button went up, anywhere. `at` as for `press`.
    pub fn release(&mut self, button: MouseButton, at: Option<CellPos>, mods: Mods) -> Vec<Act> {
        match self.held {
            Some(Held::Select { from, ctrl, moved }) if button == MouseButton::Left => {
                self.held = None;
                let mut acts = vec![Act::SelectEnd];
                if moved {
                    acts.push(Act::Copy);
                } else if let (true, Some(from)) = (ctrl, from) {
                    acts.push(Act::OpenLink(from));
                }
                acts
            }
            Some(Held::Program(held)) if held == button => {
                self.held = None;
                let Some(at) = at.or(self.reported) else {
                    return Vec::new();
                };
                let kind = MouseKind::Release;
                let button = Some(button);
                vec![Act::Report {
                    kind,
                    button,
                    at,
                    mods,
                }]
            }
            _ => Vec::new(),
        }
    }

    /// The wheel turned by `rows` (`s2_wgpu::input::wheel_rows`: negative is up) with the
    /// pointer over cell `at`.
    pub fn wheel(
        &mut self,
        rows: i32,
        at: Option<CellPos>,
        mods: Mods,
        reporting: bool,
    ) -> Vec<Act> {
        let (true, false, Some(at)) = (reporting, mods.shift, at) else {
            self.wheel_rows = 0;
            return if rows == 0 {
                Vec::new()
            } else {
                vec![Act::Scroll(rows)]
            };
        };
        // A change of direction starts afresh.
        if self.wheel_rows.signum() * rows.signum() < 0 {
            self.wheel_rows = 0;
        }
        self.wheel_rows += rows;
        let per_notch = WHEEL_LINES as i32;
        let notches = self.wheel_rows / per_notch;
        self.wheel_rows -= notches * per_notch;
        let button = Some(if notches < 0 {
            MouseButton::WheelUp
        } else {
            MouseButton::WheelDown
        });
        let kind = MouseKind::Press;
        let report = Act::Report {
            kind,
            button,
            at,
            mods,
        };
        vec![report; notches.abs().min(MAX_NOTCHES) as usize]
    }
}

/// What is left for the window to do after `perform`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    /// The picture changed (the selection).
    Redraw,
    /// This text goes to the clipboard.
    Copy(String),
    /// This link is opened (`links::open`).
    Open(String),
}

/// Does one `Act` to the session of the tab in front.
pub fn perform(act: Act, session: &mut Session) -> Option<Effect> {
    let redraw = |changed: bool| changed.then_some(Effect::Redraw);
    match act {
        Act::Report {
            kind,
            button,
            at,
            mods,
        } => {
            session.mouse(kind, button, at, mods);
            None
        }
        Act::SelectStart(at) => redraw(session.select_start(at)),
        Act::SelectDrag(at) => redraw(session.select_drag(at)),
        Act::SelectEnd => {
            session.select_end();
            None
        }
        Act::SelectClear => redraw(session.select_clear()),
        Act::Copy => session.selected_text().map(Effect::Copy),
        Act::OpenLink(at) => links::in_frame(session.frame()?, at).map(Effect::Open),
        Act::Scroll(rows) => {
            session.scroll(rows);
            None
        }
    }
}
