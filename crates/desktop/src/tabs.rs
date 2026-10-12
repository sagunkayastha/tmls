//! The tabs: one attached terminal session per tab, and which of them is in front.
//!
//! A tab is an `s2_wgpu::session::Session` (its emulator and PTY, running `tmux attach` for the
//! tab's session key) and the `Scene` its frames are drawn into. Every tab has a scene of its
//! own, so a tab keeps the rows it has built while another one is in front.
//!
//! - **Open** a key that already has a tab selects that tab.
//! - **Inactive tabs** keep running: their emulators parse on their own threads and the newest
//!   frame waits in the slot (with the dirty rows of every frame it replaced). Frames are taken
//!   only for the tab in front — `take_frames`, which passes each one to that tab's scene.
//! - **Close** drops the session: its threads end and the attach command is killed, which
//!   detaches. The tmux session lives on.
//! - **A tab whose attach command ended** (the session was killed, ssh dropped) stays where it
//!   is with its last screen and state `Ended`; no key reaches it. `reattach` starts the
//!   command again in the same tab.
//! - **Resize**: only the tab in front is told at once; the others when they are selected.
//! - **Keyboard focus** (`InputMsg::Focus`) is told to the tab in front only; a tab that goes
//!   to the back hears that it lost it.
//!
//! No window, GPU or font is needed for any of this except `take_frames`.

use std::sync::Arc;

use anyhow::Context;
use client_core::{SessionKey, tmux};
use s2_core::latency::Log;
use s2_core::{Frame, KeyEvent, Pty};
use s2_wgpu::gfx::atlas::Atlas;
use s2_wgpu::gfx::grid::{Scene, Theme};
use s2_wgpu::session::Session;

/// Makes the command a tab runs for a session key.
pub type Spawner = Box<dyn Fn(&SessionKey) -> Vec<String>>;

/// Called (on any thread, possibly spuriously) when a tab has a frame waiting or its command
/// ended.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabState {
    Live,
    /// The attach command ended. `exit` is its exit status where known; v0 never knows it
    /// (`s2_core::Emu` does not report it) and always gives `None`.
    Ended {
        exit: Option<i32>,
    },
}

struct Tab {
    key: SessionKey,
    session: Session,
    scene: Scene,
    /// This tab's session writes the latency log.
    logged: bool,
    /// `state_changes` has reported this tab as ended.
    end_seen: bool,
}

pub struct Tabs {
    tabs: Vec<Tab>,
    active: Option<usize>,
    spawner: Spawner,
    /// Columns and rows the terminal area has room for.
    grid: (u16, u16),
    waker: Option<Waker>,
    /// `--bench`: waits here for the first tab opened.
    log: Option<Log>,
    /// The terminal has the keyboard focus (the window has it, and the terminal inside it).
    focused: bool,
}

impl Tabs {
    /// Tabs that attach with `tmux::attach_argv`; `local_name` is `Config::local_name`.
    pub fn new(local_name: &str) -> Tabs {
        let local_name = local_name.to_string();
        Tabs::with_spawner(Box::new(move |key| tmux::attach_argv(key, &local_name)))
    }

    /// Tabs that run whatever `spawner` returns for a key (tests run `sh -c` scripts).
    pub fn with_spawner(spawner: Spawner) -> Tabs {
        Tabs {
            tabs: Vec::new(),
            active: None,
            spawner,
            grid: (80, 24),
            waker: None,
            log: None,
            focused: false,
        }
    }

    /// Sessions started from now on call `waker`; set it before the first `open`.
    pub fn set_waker(&mut self, waker: Waker) {
        self.waker = Some(waker);
    }

    /// `--bench`: the latency log. The first tab opened after this writes it (`logged`), and
    /// no other: key ids and frame numbers are per session, so two sessions cannot share a
    /// log. A reattached session does not write it either.
    pub fn set_log(&mut self, log: Log) {
        self.log = Some(log);
    }

    fn start(&mut self, key: &SessionKey, log: Option<Log>) -> anyhow::Result<Session> {
        let argv = (self.spawner)(key);
        let (cols, rows) = self.grid;
        let pty = Pty::spawn(&argv, cols, rows)
            .with_context(|| format!("starting {argv:?} for {}:{}", key.host, key.name))?;
        let session = Session::start(pty, cols, rows, log, false)?;
        if let Some(waker) = self.waker.clone() {
            session.set_waker(Box::new(move || waker()));
        }
        Ok(session)
    }

    /// Opens a tab for `key` at `grid` (columns, rows) and brings it to the front; a key that
    /// has a tab already just gets that tab selected. Returns the tab's index.
    pub fn open(&mut self, key: &SessionKey, grid: (u16, u16)) -> anyhow::Result<usize> {
        self.grid = grid;
        if let Some(index) = self.index_of(key) {
            self.select(index);
            return Ok(index);
        }
        let log = self.log.take();
        let logged = log.is_some();
        let session = self.start(key, log)?;
        self.tabs.push(Tab {
            key: key.clone(),
            session,
            scene: Scene::new(),
            logged,
            end_seen: false,
        });
        let index = self.tabs.len() - 1;
        self.select(index);
        Ok(index)
    }

    pub fn index_of(&self, key: &SessionKey) -> Option<usize> {
        self.tabs.iter().position(|tab| &tab.key == key)
    }

    /// The tab's session was renamed outside the app: the tab goes on under `key`. Its attach
    /// command follows the session by itself and is not started again; a reattach uses `key`.
    pub fn rekey(&mut self, index: usize, key: SessionKey) {
        self.tabs[index].key = key;
    }

    /// Brings a tab to the front (an index that does not exist changes nothing): the tab that
    /// was there loses the keyboard focus, its composition and a drag in progress; the new one
    /// gets the current grid size and, if the terminal has it, the focus.
    pub fn select(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        let (cols, rows) = self.grid;
        self.tabs[index].session.resize(cols, rows);
        if self.active == Some(index) {
            return;
        }
        if let Some(old) = self.active {
            self.leave(old);
        }
        self.active = Some(index);
        if self.focused {
            self.tabs[index].session.focus(true);
        }
    }

    /// The tab goes to the back.
    fn leave(&mut self, index: usize) {
        let session = &mut self.tabs[index].session;
        session.set_preedit("");
        session.select_end();
        if self.focused {
            session.focus(false);
        }
    }

    /// The tab to the right, wrapping round.
    pub fn next(&mut self) {
        if let Some(active) = self.active {
            self.select((active + 1) % self.tabs.len());
        }
    }

    /// The tab to the left, wrapping round.
    pub fn prev(&mut self) {
        if let Some(active) = self.active {
            self.select((active + self.tabs.len() - 1) % self.tabs.len());
        }
    }

    /// Closes a tab, which detaches: the session's emulator is dropped and its attach command
    /// killed; the tmux session lives on. When the tab in front is closed, the one that takes
    /// its place (or the last one) comes to the front.
    pub fn close(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        drop(self.tabs.remove(index));
        match self.active {
            Some(active) if active == index => {
                self.active = None;
                if !self.tabs.is_empty() {
                    self.select(index.min(self.tabs.len() - 1));
                }
            }
            Some(active) if active > index => self.active = Some(active - 1),
            _ => {}
        }
    }

    /// Starts the attach command of an ended tab again, in the same tab. A live tab is left
    /// as it is. On an error the tab stays ended.
    pub fn reattach(&mut self, index: usize) -> anyhow::Result<()> {
        if self.state(index) == TabState::Live {
            return Ok(());
        }
        let key = self.tabs[index].key.clone();
        let session = self.start(&key, None)?;
        let tab = &mut self.tabs[index];
        tab.session = session;
        tab.scene = Scene::new();
        tab.logged = false;
        tab.end_seen = false;
        if self.active == Some(index) && self.focused {
            tab.session.focus(true);
        }
        Ok(())
    }

    /// The index of the tab in front; `None` when no tab is open.
    pub fn active(&self) -> Option<usize> {
        self.active
    }

    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// Panics when `index` is not a tab, like the methods below that take one.
    pub fn key(&self, index: usize) -> &SessionKey {
        &self.tabs[index].key
    }

    pub fn state(&self, index: usize) -> TabState {
        if self.tabs[index].session.exited() {
            TabState::Ended { exit: None }
        } else {
            TabState::Live
        }
    }

    /// The state of the tab in front.
    pub fn active_state(&self) -> Option<TabState> {
        self.active.map(|index| self.state(index))
    }

    /// Whether this tab's session writes the latency log (`set_log`).
    pub fn logged(&self, index: usize) -> bool {
        self.tabs[index].logged
    }

    /// The terminal area has room for `grid` (columns, rows) now. The tab in front is resized
    /// at once, every other tab when it is selected.
    pub fn resize_all(&mut self, grid: (u16, u16)) {
        self.grid = grid;
        if let Some(active) = self.active {
            self.tabs[active].session.resize(grid.0, grid.1);
        }
    }

    /// Whether the terminal has the keyboard focus; the tab in front is told when it changes.
    pub fn set_focused(&mut self, focused: bool) {
        if focused == self.focused {
            return;
        }
        self.focused = focused;
        if let Some(active) = self.active {
            self.tabs[active].session.focus(focused);
        }
    }

    pub fn session(&self, index: usize) -> &Session {
        &self.tabs[index].session
    }

    pub fn session_mut(&mut self, index: usize) -> &mut Session {
        &mut self.tabs[index].session
    }

    /// The session of the tab in front if it is live: the only one that takes input.
    pub fn live_mut(&mut self) -> Option<&mut Session> {
        let active = self.active?;
        (self.state(active) == TabState::Live).then(|| &mut self.tabs[active].session)
    }

    /// The program in the tab in front asked for mouse reports and is there to get them. An
    /// ended tab never is: on its last screen the mouse selects.
    pub fn mouse_reporting(&self) -> bool {
        self.active.is_some_and(|active| {
            self.state(active) == TabState::Live && self.tabs[active].session.mouse_reporting()
        })
    }

    /// One key event for the tab in front, with the time it was received. Returns its key id;
    /// `None` (and nothing sent or logged) when no tab is open or its session ended.
    pub fn send_key(&mut self, t_key_ns: u128, ev: KeyEvent) -> Option<u64> {
        Some(self.live_mut()?.key(t_key_ns, ev))
    }

    /// Call when woken: the tabs whose command has ended since the last call are noted (the
    /// one in front is asked for the frame with its last output, `Session::begin_exit`).
    /// Returns whether there was one, i.e. whether the tab bar looks different now.
    pub fn state_changes(&mut self) -> bool {
        let mut changed = false;
        for tab in &mut self.tabs {
            if !tab.end_seen && tab.session.exited() {
                tab.end_seen = true;
                tab.session.begin_exit();
                changed = true;
            }
        }
        changed
    }

    /// Takes the waiting frames of the tab in front into its scene, whose origin is set to
    /// `origin` first (the terminal area's corner). Returns whether there was a new frame.
    pub fn take_frames(&mut self, origin: (f32, f32), atlas: &mut Atlas, theme: &Theme) -> bool {
        let Some(active) = self.active else {
            return false;
        };
        let tab = &mut self.tabs[active];
        tab.scene.set_origin(origin.0, origin.1);
        tab.session.take_frames(&mut tab.scene, atlas, theme)
    }

    /// The tab in front, for drawing: its session, its scene and the newest frame taken.
    /// `None` when no tab is open or the tab has no frame yet.
    pub fn front(&mut self) -> Option<Front<'_>> {
        let tab = &mut self.tabs[self.active?];
        let frame = tab.session.frame()?;
        Some(Front {
            session: &tab.session,
            scene: &mut tab.scene,
            frame,
        })
    }

    /// The scene of the tab in front (for mapping the pointer to a cell).
    pub fn front_scene(&self) -> Option<&Scene> {
        Some(&self.tabs[self.active?].scene)
    }

    /// The frames taken for the tab in front are on screen (`Session::presented`).
    pub fn presented(&mut self) {
        if let Some(active) = self.active {
            self.tabs[active].session.presented();
        }
    }
}

/// What `Tabs::front` hands to the drawing code.
pub struct Front<'a> {
    pub session: &'a Session,
    pub scene: &'a mut Scene,
    pub frame: &'a Frame,
}
