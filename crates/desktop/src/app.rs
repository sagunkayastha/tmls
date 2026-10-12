//! The winit event loop and the wgpu surface: one window with the session list and the tabs.
//!
//! This file only moves events and frames between winit, wgpu, the poller and the modules that
//! hold the logic (`tabs`, `keys`, `sidebar`, `draw` here; `input`, `session`, `marks`, `surface`, `meta`,
//! `gfx` of `s2-wgpu`); those are tested without a window. It follows `s2-wgpu`'s `app`, which
//! shows one terminal, and keeps its latency rules for the terminal of every tab:
//!
//! - the time stamp of an event is the first thing taken in `window_event`;
//! - `Marks::begin_redraw` is the first thing done in `redraw`;
//! - frames are taken only by `Tabs::take_frames`, which passes each to the tab's scene;
//! - a key reaches a session only through `keys::dispatch` → `Route::Terminal`, so keys typed
//!   while the list, a form or a menu has the focus, or at a tab whose session ended, reach
//!   no PTY and no log.
//!
//! List path: a polling thread delivers a host's listing → `UserEvent::Snapshot` →
//! `Sidebar::deliver` (which marks what finished unwatched) → a redraw that rebuilds the chrome.
//! `Alerts::observe` then compares the host's rows with its last listing: a session that started
//! to need the user is added to the bell's list and `notify-send` is started for it.
//!
//! Mouse over the terminal: `mouse::Pointer` decides whether an event is the program's (tmux
//! with `mouse on` asked for reports) or a selection, a link or a scroll of our own;
//! `mouse::perform` does it to the front tab's session; the clipboard and `xdg-open` are here.
//!
//! Request path: a form's `Request` (new, rename, kill) runs `forms::perform` on a thread of
//! its own → `UserEvent::Done` → the status line says what happened, the hosts are listed
//! again at once and the tab is opened, reopened or closed. The name a new session's folder
//! gives (`UserEvent::Derived`) and what runs in a session about to be killed
//! (`UserEvent::Running`) come the same way.
//!
//! File path: a Ctrl+click on a path in the terminal (`mouse::Effect::File`) runs
//! `viewer::open_link` on a thread of its own, which asks the session's host where the file
//! is and reads it → `UserEvent::Viewed` → text or a picture opens the panel beside the terminal
//! (`viewer::SidePanel`; the layout is made again and the tabs get the narrower grid, as on a
//! window resize), anything else goes to `xdg-open`. Only the answer to the newest click
//! counts. The panel's `Open in app` comes the same way (`viewer::open_in_app`).
//!
//! The images a session made: Alt+Shift+F or `Files` in the tab bar opens the panel on the list
//! of the front tab's session (`show_made`) — at once, saying that the host is asked — and
//! `viewer::made_while` runs on a thread like a file's click, so a list that takes long or never
//! comes holds nothing up, and its time limit ends in the status line. Lists have a counter of
//! their own (`Running::lists`) and come back as `UserEvent::Listed`: only the newest request's
//! answer counts, a click on a file never takes a list's answer away, nor a list a click's, and
//! the answer goes to the list wherever it is by then (`SidePanel::list_of`).
//! The list is asked for again by `Refresh`, when another tab comes to the front, and when a
//! listing says the session's turn ended (`snapshot`; that only starts the thread). A row
//! opens its picture the way a click in the terminal does, and that panel keeps the list to go
//! back to. A tab whose session is not Claude's runs no command.
//!
//! Frame path: an emulator thread publishes a frame (or its command ends) and calls the waker →
//! `UserEvent::Wake` → `request_redraw` → `RedrawRequested` takes the front tab's frames,
//! builds quads, submits and presents. Tabs in the back wake the loop too (that is how a tab
//! that ends in the back gets its mark); such a redraw finds nothing new and returns.
//!
//! Pacing: fifo, with `Window::pre_present_notify` before each present, so on Wayland redraws
//! follow the compositor's frame callbacks and the main thread waits in the event loop, where
//! key events get their time stamp.
//!
//! `--bench DIR`: the first tab opened writes the latency log (`Tabs::set_log`); `frame` lines
//! are written for the frames presented while that tab is in front.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow, bail};
use client_core::{Config, HostSnapshot, KITTY, Poller, Seen, SessionKey, Status, default_color};
use s2_core::bench::Sampler;
use s2_core::latency::Log;
use s2_core::now_ns;
use s2_wgpu::gfx::atlas::Atlas;
use s2_wgpu::gfx::render::{Gpu, Renderer};
use s2_wgpu::input::{self, Action};
use s2_wgpu::marks::{Marks, PresentPath, WorkDone};
use s2_wgpu::meta::{self, Refresh};
use s2_wgpu::surface::{self, Acquired, Next};
use s2_wgpu::ui::theme::{ZoomDir, zoom_step};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::window::{CursorIcon, Window, WindowId};

use crate::alerts::{self, Alerts, AlertsPanel, PanelResult, Settings, Watching};
use crate::draw::{self, Button, Chrome, ENDED_NOTICE, Hit, Layout, TabView, View};
use crate::forms::{self, Form, FormResult, MenuItem, MenuResult, Request, RowMenu};
use crate::keys::{self, Focus, Global, ListKey, PanelKey, RepeatGate, Route};
use crate::kitty;
use crate::links::{self, FileLink};
use crate::mouse::{self, Effect, Pointer};
use crate::picture::PictureRenderer;
use crate::sidebar::{self, Sidebar};
use crate::tabs::{TabState, Tabs};
use crate::theme::{SIDEBAR_COLS, SIDEBAR_MIN_COLS, Theme, host_rgb, mark_rgb};
use crate::uistate::{self, UiState};
use crate::viewer::{self, MadeBody, MadeState, Opened, SideLook, SidePanel};

/// How often the device is polled while a frame's work-done callback is outstanding; the
/// resolution of `t_work_done_ns`.
const WORK_DONE_POLL: Duration = Duration::from_micros(500);

/// How often every host is listed, and how long a listing may take before its host counts as
/// offline.
const POLL_PERIOD: Duration = Duration::from_secs(5);
const POLL_TIMEOUT: Duration = Duration::from_secs(10);

/// How long the new-session form waits after a change of the folder before it asks the host
/// for the name (tmls `create_form.py`: 0.25 s).
const LOOKUP_DELAY: Duration = Duration::from_millis(250);

/// What the command line asks for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Args {
    /// ssh hosts to list instead of those in `~/.config/tmls/hosts`.
    pub hosts: Vec<String>,
    /// `--bench DIR`: write a latency log there.
    pub bench: Option<PathBuf>,
}

#[derive(Debug)]
pub enum UserEvent {
    /// A tab has a frame waiting, or its command ended. May be spurious.
    Wake,
    /// A host's listing changed.
    Snapshot(HostSnapshot),
    /// A form's request ran; the line for the status line, or why it failed.
    Done(Request, Result<String, String>),
    /// The name a new session in `folder` on `host` gets by default.
    Derived {
        host: String,
        folder: String,
        name: String,
    },
    /// What runs in a session the user is asked about killing.
    Running(SessionKey, Vec<String>),
    /// Something done off the window's thread ended: a line for the status line, or why not.
    Said(Result<String, String>),
    /// A clicked file was found and read, or not; the number is the click's (`Running::views`).
    Viewed(u64, Result<Opened, String>),
    /// The images of a session were listed, or not; the number is the request's
    /// (`Running::lists`).
    Listed(u64, SessionKey, Result<Opened, String>),
}

/// What is open over the window and has the keys. There is at most one, so the size of a
/// form does not matter.
#[allow(clippy::large_enum_variant)]
enum Modal {
    Menu(RowMenu),
    Form(Form),
    Alerts(AlertsPanel),
}

/// Runs the app until the window is closed or the user quits. `config_dir` is `~/.config/tmls`.
pub fn run(args: Args, config_dir: PathBuf) -> anyhow::Result<()> {
    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .build()
        .context("no display to open a window on (Wayland or X11)")?;
    let mut app = App {
        args,
        config_dir,
        proxy: event_loop.create_proxy(),
        running: None,
        error: None,
    };
    event_loop.run_app(&mut app).context("event loop")?;
    app.error.map_or(Ok(()), Err)
}

struct App {
    args: Args,
    config_dir: PathBuf,
    proxy: EventLoopProxy<UserEvent>,
    running: Option<Running>,
    /// Why the app stopped, if it was not asked to.
    error: Option<anyhow::Error>,
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.running.is_some() {
            return;
        }
        match Running::start(event_loop, &self.args, &self.config_dir, self.proxy.clone()) {
            Ok(running) => self.running = Some(running),
            Err(e) => {
                self.error = Some(e);
                event_loop.exit();
            }
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        let Some(running) = &mut self.running else {
            return;
        };
        match event {
            UserEvent::Wake => {
                // Cleared before the frame is taken: a publish after this sends a new event.
                running.wake_pending.store(false, Ordering::SeqCst);
                running.woken = true;
                running.window.request_redraw();
            }
            UserEvent::Snapshot(snapshot) => running.snapshot(snapshot),
            UserEvent::Done(request, result) => running.done(request, result),
            UserEvent::Said(said) => running.say(said),
            UserEvent::Derived { host, folder, name } => {
                if let Some((Modal::Form(Form::New(new)), _)) = &mut running.modal {
                    new.derived(&host, &folder, &name);
                    running.invalidate();
                }
            }
            UserEvent::Running(key, commands) => {
                if let Some((Modal::Form(form), _)) = &mut running.modal {
                    form.set_running(&key, commands);
                    running.invalidate();
                }
            }
            UserEvent::Viewed(click, result) => running.viewed(click, result),
            UserEvent::Listed(load, key, result) => running.listed(load, key, result),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        // `t_key_ns`: the first thing done with any event, before it is even looked at.
        let t_event_ns = now_ns();
        let Some(running) = &mut self.running else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => {
                let cause = if std::mem::take(&mut running.woken) {
                    Redraw::Woken
                } else {
                    Redraw::Forced
                };
                if let Err(e) = running.redraw(cause) {
                    self.error = Some(e);
                    event_loop.exit();
                }
            }
            WindowEvent::Resized(size) => running.resized(size),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => running.rescaled(scale_factor),
            WindowEvent::Occluded(false) => running.invalidate(),
            WindowEvent::Focused(focused) => {
                // For a test driver: it sends keys only to the window that has the focus.
                crate::errln(format_args!("tmls-rs: focus {focused}"));
                running.window_focused = focused;
                running.sync_focus();
            }
            WindowEvent::ModifiersChanged(mods) => running.mods = mods.state(),
            WindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } => {
                if running.key(t_event_ns, &event) == After::Quit {
                    event_loop.exit();
                }
            }
            WindowEvent::Ime(ime) => running.ime(t_event_ns, &ime),
            WindowEvent::CursorMoved { position, .. } => running.pointer_moved(position),
            WindowEvent::MouseInput { state, button, .. } => {
                running.button(state, button);
                if running.quit {
                    event_loop.exit();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => running.wheel(&delta),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(running) = &mut self.running else {
            return;
        };
        let now = Instant::now();
        if running.retry_at.is_some_and(|at| now >= at) {
            running.retry_at = None;
            running.invalidate();
        }
        if running.meta.first_try.is_some() {
            // The window may have entered its monitor since the first present.
            running.write_meta(false);
        }
        let poll = running.settle_marks().then_some(now + WORK_DONE_POLL);
        let next = [poll, running.retry_at, running.meta_deadline()]
            .into_iter()
            .flatten()
            .min();
        event_loop.set_control_flow(next.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(running) = self.running.take() {
            running.shutdown();
        }
    }
}

/// Why a redraw runs.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Redraw {
    /// Requested after a wake-up: nothing to do if nothing changed after all.
    Woken,
    /// The window system asked, or the app must draw whatever it has.
    Forced,
}

/// What the event loop has to do after a key.
#[derive(Clone, Copy, PartialEq, Eq)]
enum After {
    Stay,
    Quit,
}

/// What goes into the `meta` line besides the values read when it is written.
struct Meta {
    written: bool,
    /// When `write_meta` first ran: the refresh rate is waited for from then (`meta::META_WAIT`).
    first_try: Option<Instant>,
}

/// The window's title with no tab open.
const TITLE: &str = "tmls-rs";

struct Running {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    gpu: Gpu,
    renderer: Renderer,
    /// The picture of the file panel, while it shows one: its texture, and the pass that
    /// draws it after the renderer's. Not used at all for a frame without one.
    picture: PictureRenderer,
    atlas: Atlas,
    theme: Theme,
    /// Where everything is, for the current size, scale and font size.
    layout: Layout,
    /// The quads of everything but the terminal, rebuilt only when something they show
    /// changed.
    chrome: Chrome,
    /// What `chrome` draws besides the session list; put together again on every redraw that
    /// is `dirty`.
    view: View,

    cfg: Config,
    tabs: Tabs,
    /// What has the keyboard focus inside the window.
    focus: Focus,
    gate: RepeatGate,
    /// The session list.
    sidebar: Sidebar,
    /// Which sessions finished while their tab was not in front.
    seen: Seen,
    /// Which sessions started to need the user; the bell's count.
    alerts: Alerts,
    /// The last thing that went wrong (a tab that could not be opened), for the status line.
    problem: Option<String>,
    /// What a request did ("created x on host"), for the status line until the next key.
    note: Option<String>,
    /// The open form or menu, and the window pixel a menu was opened at.
    modal: Option<(Modal, Option<(u32, u32)>)>,
    /// What had the keys before `modal` opened.
    focus_before: Focus,
    /// A session just made or renamed here that the listing does not have yet: the list's
    /// cursor goes to it when it is listed, unless the user moved the cursor first.
    select_when_listed: Option<SessionKey>,
    /// Counts the new-session form's lookups: an answer is asked for only by the latest.
    lookups: Arc<AtomicU64>,
    /// For the threads that run requests.
    proxy: EventLoopProxy<UserEvent>,
    /// The polling threads.
    poller: Poller,

    log: Option<Log>,
    marks: Option<Marks<Log>>,
    sampler: Option<Sampler>,
    meta: Meta,
    clipboard: Option<arboard::Clipboard>,
    mods: ModifiersState,
    pointer: (f32, f32),
    wheel_carry: f32,
    /// The mouse over the terminal: which button is held, and for whom.
    mouse: Pointer,
    /// A `Wake` event is on its way; the waker sends no second one.
    wake_pending: Arc<AtomicBool>,
    /// The pending `RedrawRequested` was asked for by a wake-up.
    woken: bool,
    /// Something other than a new frame changed the picture (the list, a tab, size, scale).
    dirty: bool,
    /// Where the IME was last told the text cursor is (window pixels).
    ime_at: Option<(f32, f32)>,
    /// The window has the keyboard focus.
    window_focused: bool,
    /// What the window was last told about the input method (`Window::set_ime_allowed`).
    ime_allowed: bool,
    /// A redraw got no texture and is to be tried again at this time.
    retry_at: Option<Instant>,
    /// The ✕ at the foot of the list was clicked: the event loop is to stop.
    quit: bool,
    /// The list's right edge is being dragged.
    dragging_separator: bool,
    /// The file panel's divider is being dragged: how far right of the divider's left edge
    /// the pointer was when the button went down (the handle is wider than the line).
    dragging_divider: Option<f32>,
    /// The file panel's width in columns, once its divider was dragged (now or in an earlier
    /// run); until then a panel opens with `draw::default_panel_cols`.
    panel_cols: Option<usize>,
    /// The dragged widths as they are in `ui_path`, and where that is (`uistate`).
    ui: UiState,
    ui_path: Option<PathBuf>,
    /// The pointer is drawn as the resize arrows.
    resize_cursor: bool,
    /// The sketchpad hub's address (`~/.config/tmls/sketchpad`), if there is one.
    sketchpad: Option<String>,
    /// The file panel beside the terminal, while one is open.
    side: Option<SidePanel>,
    /// Counts the clicks on files: the answer to the latest is the only one taken.
    views: Arc<AtomicU64>,
    /// Counts the requests for the list of a session's images, apart from the clicks: the
    /// answer to the latest is the only one taken, and neither kind overtakes the other.
    lists: Arc<AtomicU64>,
    /// The click that opens a picture from the list of a session's images: its panel keeps
    /// the list to go back to.
    back_for: Option<u64>,
    /// Where copies of other hosts' files go to be opened here.
    view_cache: Option<PathBuf>,
}

impl Running {
    fn start(
        event_loop: &ActiveEventLoop,
        args: &Args,
        config_dir: &std::path::Path,
        proxy: EventLoopProxy<UserEvent>,
    ) -> anyhow::Result<Running> {
        let attributes = Window::default_attributes()
            .with_title(TITLE)
            .with_inner_size(LogicalSize::new(1200.0, 720.0));
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .context("create window")?,
        );
        // Nothing is open yet and the keys are the list's, which takes no composed text.
        window.set_ime_allowed(false);

        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle_from_env(
                Box::new(event_loop.owned_display_handle()),
            ));
        let surface = instance
            .create_surface(window.clone())
            .context("create surface")?;
        let request = |force_fallback_adapter| {
            let options = wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                force_fallback_adapter,
                ..Default::default()
            };
            pollster::block_on(instance.request_adapter(&options)).ok()
        };
        let adapter = request(false)
            .or_else(|| request(true))
            .ok_or_else(|| anyhow!("no wgpu adapter can draw to this window"))?;
        let info = adapter.get_info();
        let caps = surface.get_capabilities(&adapter);
        let gpu = Gpu::from_adapter(instance, adapter)?;

        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&gpu.adapter, size.width.max(1), size.height.max(1))
            .ok_or_else(|| anyhow!("the adapter cannot present to this window"))?;
        // Colours are sRGB bytes blended as they are (see `gfx::render`): a non-sRGB format.
        config.format = caps
            .formats
            .iter()
            .copied()
            .find(|format| !format.is_srgb())
            .unwrap_or(config.format);
        // Every surface offers fifo.
        config.present_mode = wgpu::PresentMode::Fifo;
        if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
            config.alpha_mode = wgpu::CompositeAlphaMode::Opaque;
        }
        // The shortest queue of frames waiting for the display the surface allows.
        config.desired_maximum_frame_latency = 1;
        surface.configure(&gpu.device, &config);

        // The widths dragged in an earlier run. The layout keeps them within the window.
        let ui_path = uistate::path(
            std::env::var("XDG_CONFIG_HOME").ok().as_deref(),
            std::env::var("HOME").ok().as_deref(),
        );
        let ui = ui_path.as_deref().map(uistate::load).unwrap_or_default();
        let mut theme = Theme::default();
        if let Some(cols) = ui.sidebar_cols {
            theme.sidebar_cols = cols.max(SIDEBAR_MIN_COLS);
        }
        let mut atlas = Atlas::new(theme.font.clone(), window.scale_factor() as f32);
        atlas.set_max_height(gpu.device.limits().max_texture_dimension_2d);
        let layout = draw::layout(
            (config.width, config.height),
            atlas.scale(),
            &atlas.metrics(),
            &theme,
        );
        let renderer = Renderer::new(&gpu.device, config.format);
        let picture = PictureRenderer::new(&gpu.device, config.format);

        let log = match &args.bench {
            Some(dir) => {
                Some(Log::create(dir).with_context(|| format!("--bench {}", dir.display()))?)
            }
            None => None,
        };
        let cfg = Config::load(config_dir, &args.hosts);
        let sidebar = Sidebar::new(&cfg, config_dir.to_path_buf());
        let mut tabs = Tabs::new(&cfg.local_name);
        let wake_pending = Arc::new(AtomicBool::new(false));
        let (pending, wake_proxy) = (wake_pending.clone(), proxy.clone());
        let poll_proxy = proxy.clone();
        tabs.set_waker(Arc::new(move || {
            if !pending.swap(true, Ordering::SeqCst) {
                let _ = wake_proxy.send_event(UserEvent::Wake);
            }
        }));
        if let Some(log) = &log {
            tabs.set_log(log.clone());
        }
        // One ssh connection per host, kept 10 minutes; TMLS_RS_NO_CONTROLMASTER=1 turns it off
        // (the keepalive stays). Before the poller: its first listing is the first ssh.
        if client_core::tmux::control_wanted(
            std::env::var_os("TMLS_RS_NO_CONTROLMASTER").as_deref(),
        ) {
            let dir = client_core::tmux::control_dir(
                std::env::var_os("XDG_RUNTIME_DIR").as_deref(),
                std::env::var_os("HOME").as_deref(),
            );
            match dir.map(|dir| client_core::tmux::use_control_master(&dir).map(|()| dir)) {
                Some(Ok(dir)) => crate::errln(format_args!(
                    "tmls-rs: ssh shares one connection per host ({})",
                    dir.display()
                )),
                Some(Err(e)) => {
                    crate::errln(format_args!("tmls-rs: ssh without shared connections: {e}"))
                }
                None => crate::errln(format_args!(
                    "tmls-rs: ssh without shared connections: no place for the sockets"
                )),
            }
        }
        let poller = Poller::start(
            &cfg,
            POLL_PERIOD,
            POLL_TIMEOUT,
            Box::new(move |snapshot| {
                let _ = poll_proxy.send_event(UserEvent::Snapshot(snapshot));
            }),
        );

        let (cols, rows) = layout.grid;
        crate::errln(format_args!(
            "tmls-rs: adapter {:?} / {} / {} {}; format {:?}; present fifo; grid {cols}x{rows}; hosts {:?}",
            info.backend, info.name, info.driver, info.driver_info, config.format, cfg.hosts,
        ));
        Ok(Running {
            marks: log.clone().map(Marks::new),
            sampler: log.clone().map(Sampler::start),
            meta: Meta {
                written: false,
                first_try: None,
            },
            window,
            surface,
            config,
            gpu,
            renderer,
            picture,
            atlas,
            theme,
            layout,
            chrome: Chrome::new(),
            view: View::default(),
            cfg,
            tabs,
            // Nothing is open yet: the keys are the list's.
            focus: Focus::Sidebar,
            gate: RepeatGate::default(),
            sidebar,
            seen: Seen::new(),
            alerts: Alerts::new(Settings::load(config_dir)),
            problem: None,
            note: None,
            modal: None,
            focus_before: Focus::Sidebar,
            select_when_listed: None,
            lookups: Arc::new(AtomicU64::new(0)),
            proxy,
            poller,
            log,
            clipboard: None,
            mods: ModifiersState::empty(),
            pointer: (-1.0, -1.0),
            wheel_carry: 0.0,
            mouse: Pointer::default(),
            wake_pending,
            woken: false,
            dirty: true,
            ime_at: None,
            window_focused: false,
            ime_allowed: false,
            retry_at: None,
            quit: false,
            dragging_separator: false,
            dragging_divider: None,
            panel_cols: ui.panel_cols,
            ui,
            ui_path,
            resize_cursor: false,
            sketchpad: Config::sketchpad(config_dir),
            side: None,
            views: Arc::new(AtomicU64::new(0)),
            lists: Arc::new(AtomicU64::new(0)),
            back_for: None,
            view_cache: viewer::cache_dir(
                std::env::var_os("XDG_CACHE_HOME").as_deref(),
                std::env::var_os("HOME").as_deref(),
            ),
        })
    }

    /// The picture changed without a new frame: draw at the next opportunity.
    fn invalidate(&mut self) {
        self.dirty = true;
        self.window.request_redraw();
    }

    fn resized(&mut self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        (self.config.width, self.config.height) = (size.width, size.height);
        self.surface.configure(&self.gpu.device, &self.config);
        self.relayout();
    }

    /// A new scale factor (HiDPI, another monitor): every glyph is rasterised again at the
    /// new size and the layout is made for it.
    fn rescaled(&mut self, scale: f64) {
        if self.atlas.set_scale(scale as f32) {
            crate::errln(format_args!("tmls-rs: scale factor {scale}"));
            self.relayout();
        }
    }

    /// Ctrl+= / Ctrl+−: one step of the font size; like a scale change for the atlas.
    fn zoom(&mut self, dir: ZoomDir) {
        let size = zoom_step(self.atlas.font_size(), dir);
        if self.atlas.set_font_size(size) {
            self.relayout();
        }
    }

    /// The list's right edge follows the pointer, a whole column at a time, between
    /// `SIDEBAR_MIN_COLS` and half the window. The terminals get their new size at once.
    fn drag_separator(&mut self, x: f32) {
        let cell_w = self.layout.cell.0.max(1.0);
        let most = ((self.config.width as f32 / 2.0) / cell_w).floor() as usize;
        let cols = ((x / cell_w).round().max(0.0) as usize)
            .clamp(SIDEBAR_MIN_COLS, most.max(SIDEBAR_MIN_COLS));
        if cols != self.theme.sidebar_cols {
            self.theme.sidebar_cols = cols;
            self.relayout();
        }
    }

    /// The file panel's divider follows the pointer, a whole column at a time; the layout
    /// keeps `PANEL_MIN_COLS` columns on both sides of it. The terminals get their new size at
    /// once.
    fn drag_divider(&mut self, x: f32, offset: f32) {
        if self.side.is_none() || self.layout.side.w <= 0.0 {
            return;
        }
        let asked = self.layout.divider_cols(x, offset);
        // As many of them as the layout gives.
        let cols = self.layout_for(Some(asked)).side_cols;
        if cols != 0 && cols != self.layout.side_cols {
            self.panel_cols = Some(cols);
            self.relayout();
        }
    }

    /// A drag of the list's edge or of the panel's divider ended: the widths that were
    /// dragged, now or before, are written to `~/.config/tmls-rs/ui` if the drag changed one
    /// (a click on a handle does not). A file that cannot be written shows in the status line.
    fn save_widths(&mut self) {
        let list = self.theme.sidebar_cols;
        let dragged = UiState {
            sidebar_cols: match list == self.ui.sidebar_cols.unwrap_or(SIDEBAR_COLS) {
                true => self.ui.sidebar_cols,
                false => Some(list),
            },
            panel_cols: self.panel_cols,
        };
        if dragged == self.ui {
            return;
        }
        let Some(path) = self.ui_path.clone() else {
            return;
        };
        let saved = uistate::save(&path, &dragged)
            .with_context(|| format!("the widths were not saved: {}", path.display()));
        // Saved: nothing to say, and what the status line says of something else stays.
        match saved {
            Ok(()) => self.ui = dragged,
            Err(_) => self.report(saved),
        }
    }

    /// The layout of the window as it is, with a file panel of `side` columns or none.
    fn layout_for(&self, side: Option<usize>) -> Layout {
        draw::layout_with(
            (self.config.width, self.config.height),
            self.atlas.scale(),
            &self.atlas.metrics(),
            &self.theme,
            side,
        )
    }

    /// Window size, scale factor, font size, the list's width or the panel's changed, or the
    /// file panel opened or closed: lays the window out again and tells the tabs how many
    /// cells fit now.
    fn relayout(&mut self) {
        // A panel whose divider was never dragged has half the columns, whatever the window.
        let side = self.side.is_some().then(|| {
            self.panel_cols.unwrap_or_else(|| {
                draw::default_panel_cols(
                    (self.config.width, self.config.height),
                    self.atlas.scale(),
                    &self.atlas.metrics(),
                    &self.theme,
                )
            })
        });
        self.layout = self.layout_for(side);
        self.tabs.resize_all(self.layout.grid);
        // A window made too narrow for the panel: the keys do not stay with a list that is
        // not drawn.
        self.settle_panel_focus();
        self.invalidate();
    }

    /// A host's new listing, in the order the poller delivers them. The session of the tab in
    /// front is being watched: it gets no "finished unwatched" mark.
    fn snapshot(&mut self, snapshot: HostSnapshot) {
        let front = self.tabs.active().map(|index| self.tabs.key(index));
        let (host, online) = (snapshot.host.clone(), snapshot.online);
        // The session whose images the panel lists, and what it was doing until now.
        let images = self.side.as_ref().and_then(SidePanel::made_body);
        let images = images.map(|body| (body.key.clone(), body.state()));
        let was = images
            .as_ref()
            .and_then(|(key, _)| self.sidebar.status(key));
        self.sidebar.deliver(&mut self.seen, snapshot, front);
        // The host's sessions as the list has them now, marks included.
        let listed = self.sidebar.rows().iter().filter_map(|row| match row {
            sidebar::Row::Session(info) if info.key.host == host => Some(info),
            _ => None,
        });
        let watching = Watching {
            front,
            window_focused: self.window_focused,
        };
        let notes = self
            .alerts
            .observe(&host, online, listed, watching, Instant::now());
        alerts::send(&notes, &mut forms::spawn_detached);
        if let Some(key) = &self.select_when_listed
            && self.sidebar.status(key).is_some()
        {
            self.sidebar.select_key(key);
            self.select_when_listed = None;
        }
        if let Some((key, state)) = images {
            self.relist_made(&key, state, was);
        }
        self.invalidate();
    }

    /// After a listing: the panel's list of the images of the session `key` is asked for
    /// again if the session's turn just ended (it `was` something else and is done or idle
    /// now: what it made is there), or if the tab had no Claude session and has one now (or
    /// the other way round). A click on a file that waits for its answer is not in the way:
    /// lists are counted apart. Only starts the thread: nothing here waits for a host.
    fn relist_made(&mut self, key: &SessionKey, state: MadeState, was: Option<Status>) {
        let front = self.tabs.active().map(|index| self.tabs.key(index));
        if front != Some(key) {
            return;
        }
        let now = self.sidebar.status(key);
        let ended = now != was && matches!(now, Some(Status::Unseen | Status::Idle));
        let has_claude = self.session_id(key).is_some();
        // (A session that is not listed, its host being away, has not stopped being Claude's.)
        let changed = now.is_some() && has_claude != (state != MadeState::NoClaude);
        if (ended && has_claude) || changed {
            self.show_made();
        }
    }

    /// Claude's id of the session `key`, as its host last listed it.
    fn session_id(&self, key: &SessionKey) -> Option<String> {
        self.sidebar.rows().iter().find_map(|row| match row {
            sidebar::Row::Session(info) if info.key == *key => info.session_id.clone(),
            _ => None,
        })
    }

    /// What `chrome` draws besides the session list, from the state as it is now.
    fn build_view(&mut self) -> View {
        let panel = self.modal.as_mut().map(|(modal, at)| match modal {
            Modal::Menu(menu) => (menu.panel(), *at),
            Modal::Form(form) => (form.panel(), *at),
            Modal::Alerts(alerts) => (alerts.panel(), *at),
        });
        let status_of = |key: &SessionKey| {
            let status = self.sidebar.status(key)?;
            Some((status.mark(), mark_rgb(status)))
        };
        let tabs = (0..self.tabs.len())
            .map(|index| {
                let key = self.tabs.key(index);
                let host = self.cfg.host_colors.get(&key.host).copied();
                TabView {
                    key: key.clone(),
                    mark: status_of(key),
                    host: host_rgb(host.unwrap_or_else(|| default_color(&key.host))),
                    ended: self.tabs.state(index) != TabState::Live,
                }
            })
            .collect();
        let active = self.tabs.active();
        let ended = matches!(self.tabs.active_state(), Some(TabState::Ended { .. }));
        let status = match (self.problem.as_ref().or(self.note.as_ref()), active) {
            (Some(said), _) => said.clone(),
            (None, Some(index)) => {
                let key = self.tabs.key(index);
                let state = if ended { " · ended" } else { "" };
                format!("{} · {}{state}", key.host, key.name)
            }
            (None, None) => "no session open — Enter in the list attaches".to_string(),
        };
        View {
            sidebar_focused: self.focus == Focus::Sidebar,
            tabs,
            active,
            status,
            notice: ended.then(|| ENDED_NOTICE.to_string()),
            panel,
            sketch: self.sketchpad.is_some(),
            unread: self.alerts.unread(),
            side: self.side.as_ref().map(|side| {
                let l = &self.layout;
                side.view(l.side_lines, l.side_cols, l.side_body)
            }),
            side_focused: self.focus == Focus::Panel,
        }
    }

    fn key(&mut self, t_key_ns: u128, event: &KeyEvent) -> After {
        let pressed = event.state == ElementState::Pressed;
        let action = input::key_action(
            &event.logical_key,
            event.text.as_deref(),
            self.mods,
            pressed,
            event.repeat,
        );
        if pressed && self.note.take().is_some() {
            self.invalidate();
        }
        // The panel has no use for a copy or a paste: they are the terminal's, with the keys.
        if self.focus == Focus::Panel && matches!(action, Action::Copy | Action::Paste) {
            self.leave_panel();
        }
        let ev = match action {
            Action::Send(ev) => ev,
            Action::Paste if self.focus == Focus::Form => {
                self.paste_into_form();
                return After::Stay;
            }
            Action::Copy | Action::Paste if self.focus != Focus::Terminal => return After::Stay,
            Action::Copy => {
                self.copy();
                return After::Stay;
            }
            Action::Paste => {
                self.paste();
                return After::Stay;
            }
            Action::Ignore => return After::Stay,
        };
        // A key the panel does not bind is the terminal's: the keys go back there, and this
        // one with them, so nothing typed is lost.
        if self.focus == Focus::Panel && keys::leaves_panel(&ev) {
            self.leave_panel();
        }
        if let Some(text) = keys::copy_instead(&ev, self.focus, &mut self.gate, &mut self.tabs) {
            self.set_clipboard(text);
            self.invalidate();
            return After::Stay;
        }
        // A key for the terminal is sent in there, and nowhere else.
        let (route, ev) = keys::dispatch(t_key_ns, ev, self.focus, &mut self.gate, &mut self.tabs);
        match route {
            Route::Global(Global::Quit) => return After::Quit,
            Route::Global(global) => self.global(global),
            Route::Sidebar => {
                if let Some(key) = ev.as_ref().and_then(keys::list_key) {
                    self.list_key(key);
                }
            }
            Route::Form => {
                if let Some(ev) = ev {
                    self.form_key(&ev);
                }
            }
            Route::Panel => {
                if let Some(key) = ev.as_ref().and_then(keys::panel_key) {
                    self.panel_key(key);
                }
            }
            Route::Terminal | Route::Drop => {}
        }
        After::Stay
    }

    /// Opens a form or a menu over the window and gives it the keys.
    fn open_modal(&mut self, modal: Modal, at: Option<(u32, u32)>) {
        if self.modal.is_none() {
            self.focus_before = self.focus;
        }
        self.modal = Some((modal, at));
        self.set_focus(Focus::Form);
    }

    /// Closes the form or menu; the keys go back to where they were.
    fn close_modal(&mut self) {
        if self.modal.take().is_none() {
            return;
        }
        let panel = self.panel_takes_keys();
        let back = match self.focus_before {
            Focus::Panel if panel => Focus::Panel,
            Focus::Terminal | Focus::Panel if !self.tabs.is_empty() => Focus::Terminal,
            _ => Focus::Sidebar,
        };
        self.set_focus(back);
    }

    /// The new-session form for `host`.
    fn new_session_form(&mut self, host: &str) {
        let form = Form::new_session(&self.cfg.hosts, host, &self.cfg.presets);
        self.open_modal(Modal::Form(form), None);
    }

    /// A key for the open form or menu.
    fn form_key(&mut self, ev: &s2_core::KeyEvent) {
        match &mut self.modal {
            Some((Modal::Menu(menu), _)) => {
                let result = menu.key(ev);
                self.menu_result(result);
            }
            Some((Modal::Form(form), _)) => {
                let result = form.key(ev);
                self.form_result(result);
            }
            Some((Modal::Alerts(alerts), _)) => {
                let result = alerts.key(ev);
                self.alerts_result(result);
            }
            None => {}
        }
        self.invalidate();
    }

    /// Opens the list of alerts; they count as read from now on.
    fn open_alerts(&mut self) {
        let panel = self.alerts.open(Instant::now());
        self.open_modal(Modal::Alerts(panel), None);
    }

    fn alerts_result(&mut self, result: PanelResult) {
        match result {
            PanelResult::Pending => {}
            PanelResult::Closed => self.close_modal(),
            PanelResult::Jump(key) => {
                self.close_modal();
                let listed = self.sidebar.status(&key).is_some();
                let has_tab = self.tabs.index_of(&key).is_some();
                let host_online = self.sidebar.rows().iter().any(|row| {
                    matches!(row, sidebar::Row::Host { host, online: true, .. } if *host == key.host)
                });
                match alerts::jump(&key, listed, has_tab, host_online) {
                    Ok(()) => self.open(&key),
                    // Not a problem to keep in the status line: gone with the next key.
                    Err(gone) => self.say(Ok(gone)),
                }
            }
        }
        self.invalidate();
    }

    fn menu_result(&mut self, result: MenuResult) {
        let Some((Modal::Menu(menu), _)) = &self.modal else {
            return;
        };
        let key = menu.key.clone();
        match result {
            MenuResult::Pending => {}
            MenuResult::Closed => self.close_modal(),
            MenuResult::Pick(MenuItem::Rename) => {
                self.modal = Some((Modal::Form(Form::rename(key)), None));
            }
            MenuResult::Pick(MenuItem::Kill) => {
                // The question is asked at once; what runs in the session is added to it
                // when the host has said.
                self.modal = Some((Modal::Form(Form::confirm_kill(key.clone())), None));
                let (proxy, local) = (self.proxy.clone(), self.cfg.local_name.clone());
                std::thread::spawn(move || {
                    let commands = forms::running_commands(&key, &local, &forms::run_tmux);
                    let _ = proxy.send_event(UserEvent::Running(key, commands));
                });
            }
            MenuResult::Pick(MenuItem::OpenTerminal) => {
                self.close_modal();
                let terminal = std::env::var("TERMINAL").ok();
                let said = forms::open_terminal(
                    &key,
                    &self.cfg.local_name,
                    terminal.as_deref(),
                    &forms::on_path,
                    &mut forms::spawn_detached,
                );
                self.say(said);
            }
            MenuResult::Pick(MenuItem::CopyAttach) => {
                self.close_modal();
                self.copy_attach(&key);
            }
        }
        self.invalidate();
    }

    fn form_result(&mut self, result: FormResult) {
        match result {
            FormResult::Pending => self.lookup_name(),
            FormResult::Cancelled => self.close_modal(),
            FormResult::Submit(mut request) => {
                self.close_modal();
                if let Request::New(new) = &mut request {
                    // At the view's size, so what starts in it lays out for this window.
                    new.size = Some(self.layout.grid);
                }
                let (proxy, local) = (self.proxy.clone(), self.cfg.local_name.clone());
                std::thread::spawn(move || {
                    let result = forms::perform(&request, &local, &forms::run_tmux);
                    let _ = proxy.send_event(UserEvent::Done(request, result));
                });
            }
        }
        self.invalidate();
    }

    /// Asks the host for the name the new-session form's folder gives, a moment after the
    /// last change; an older question that has not been asked yet is dropped.
    fn lookup_name(&mut self) {
        let Some((Modal::Form(Form::New(new)), _)) = &mut self.modal else {
            return;
        };
        let Some((host, folder)) = new.lookup() else {
            return;
        };
        let lookups = self.lookups.clone();
        let mine = lookups.fetch_add(1, Ordering::SeqCst) + 1;
        let (proxy, local) = (self.proxy.clone(), self.cfg.local_name.clone());
        std::thread::spawn(move || {
            std::thread::sleep(LOOKUP_DELAY);
            if lookups.load(Ordering::SeqCst) != mine {
                return;
            }
            if let Some(name) = forms::derived_name(&host, &folder, &local, &forms::run_tmux) {
                let _ = proxy.send_event(UserEvent::Derived { host, folder, name });
            }
        });
    }

    /// A request ran: says so in the status line, lists the hosts again, and opens the new
    /// session's tab, reopens a renamed session's tab under the new name, or closes a killed
    /// session's tab.
    /// A rename or kill asked from the list leaves the keys in the list
    /// (`keys::list_keeps_keys`).
    fn done(&mut self, request: Request, result: Result<String, String>) {
        let ok = result.is_ok();
        self.say(result);
        if !ok {
            return;
        }
        self.poller.refresh_now();
        let keys_were = keys::keys_at(self.focus, self.focus_before, self.modal.is_some());
        let list_keeps_keys = keys::list_keeps_keys(&request, keys_were);
        match request {
            Request::New(new) => {
                let key = SessionKey {
                    host: new.host,
                    name: new.name,
                };
                self.select_when_listed = Some(key.clone());
                // A form or menu opened since then keeps the keys (`tab_changed`): the tab
                // gets them when it closes.
                self.open(&key);
            }
            Request::Rename(old, name) => {
                let key = SessionKey {
                    host: old.host.clone(),
                    name,
                };
                // The ★ follows the name. A `pins` file that cannot be written is not worth
                // hiding that the rename worked: it goes to stderr only.
                self.sidebar.renamed(&old, &key);
                if let Err(e) = self.sidebar.take_pin_error() {
                    crate::errln(format_args!("tmls-rs: {e:#}"));
                }
                if let Some(index) = self.tabs.index_of(&old) {
                    let front = self.tabs.active() == Some(index);
                    self.close_tab(index);
                    let back = self.tabs.active();
                    self.select_when_listed = Some(key.clone());
                    // Also with a form or menu open: the tab is never left closed.
                    self.open(&key);
                    // A tab that was in the back stays there.
                    if let (false, Some(back)) = (front, back) {
                        self.tabs.select(back);
                        self.tab_changed();
                    }
                }
            }
            Request::Kill(key) => {
                if let Some(index) = self.tabs.index_of(&key) {
                    self.close_tab(index);
                }
                if list_keeps_keys {
                    // The cursor stays on the row that goes; the next listing moves it to a
                    // neighbour, not to the row of the tab that came to the front.
                    self.sidebar.select_key(&key);
                }
            }
        }
        if list_keeps_keys {
            // Asked from the list: the tab that came to the front does not take the keys.
            if self.modal.is_some() {
                self.focus_before = Focus::Sidebar;
            } else if self.focus != Focus::Sidebar {
                self.set_focus(Focus::Sidebar);
            }
        }
    }

    /// What an action did, for the status line: a line to show until the next key, or what
    /// went wrong.
    fn say(&mut self, said: Result<String, String>) {
        match said {
            Ok(note) => (self.note, self.problem) = (Some(note), None),
            Err(problem) => {
                crate::errln(format_args!("tmls-rs: {problem}"));
                (self.note, self.problem) = (None, Some(problem));
            }
        }
        self.invalidate();
    }

    /// A Ctrl+click on a file path in the front tab: its host is asked for the file on a
    /// thread; the answer comes back as `UserEvent::Viewed`.
    fn open_file(&mut self, link: FileLink) {
        let Some(key) = self.tabs.active().map(|index| self.tabs.key(index).clone()) else {
            return;
        };
        self.say(Ok(format!("opening {} on {} …", link.path, key.host)));
        self.view(move |local, cache, wanted| {
            viewer::open_link_while(&key, &link, local, cache, &viewer::run, wanted)
        });
    }

    /// Alt+Shift+F, or a click on `Files`: closes the panel if it is open, else opens it on
    /// the list of the images the front tab's session made.
    fn toggle_files(&mut self) {
        match self.side {
            Some(_) => self.close_side(),
            None => self.show_made(),
        }
    }

    /// Puts the list of the images the front tab's session made in the panel and asks the
    /// session's host for it on a thread; the answer comes back as `UserEvent::Listed`, and
    /// counts only if no list was asked for since. The panel shows at once: a list of that
    /// session that is there stays until the new one comes, else it says that the host is
    /// asked. A session that is not Claude's has no transcript to read: the panel says so
    /// and no command runs.
    fn show_made(&mut self) {
        let Some(key) = self.tabs.active().map(|index| self.tabs.key(index).clone()) else {
            return self.say(Ok("no session open: no images to list".to_string()));
        };
        let id = self.session_id(&key);
        let listed = self.side.as_ref().and_then(SidePanel::made_body);
        let kept = id.is_some()
            && listed.is_some_and(|body| body.key == key && body.state() == MadeState::Listed);
        if !kept {
            let body = match id {
                Some(_) => MadeBody::loading(key.clone()),
                None => MadeBody::no_claude(key.clone()),
            };
            // Whatever the panel showed, it shows no picture now: its texture is freed.
            self.picture.set(&self.gpu.device, &self.gpu.queue, None);
            if !self.show_side(SidePanel::made(body)) {
                self.lists.fetch_add(1, Ordering::SeqCst);
                return self.say(Ok(viewer::TOO_NARROW.to_string()));
            }
        }
        // The request's number: an answer to an older one is to a list this one took the
        // place of, also when nothing is asked now.
        let load = self.lists.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some(id) = id {
            let (proxy, local) = (self.proxy.clone(), self.cfg.local_name.clone());
            let lists = self.lists.clone();
            std::thread::spawn(move || {
                let wanted = || lists.load(Ordering::SeqCst) == load;
                let result = viewer::made_while(&key, &id, &local, &viewer::run, &wanted);
                let _ = proxy.send_event(UserEvent::Listed(load, key, result));
            });
        }
        self.invalidate();
    }

    /// The answer to the request for a session's images: it goes to that session's list,
    /// which the panel shows or keeps behind a picture opened from it. A host that did not
    /// answer (its time limit, a dead link) is said in the list and, with the reason, in the
    /// status line. An answer to a request that is not the latest is dropped; one whose list
    /// is gone (a file took the panel) has nowhere to go.
    fn listed(&mut self, load: u64, key: SessionKey, result: Result<Opened, String>) {
        if load != self.lists.load(Ordering::SeqCst) {
            return;
        }
        let Some(list) = self.side.as_mut().and_then(|side| side.list_of(&key)) else {
            return;
        };
        let problem = match result {
            Ok(Opened::Made { files, .. }) => {
                list.set_files(files);
                None
            }
            // `viewer::made_while` gives nothing else.
            Ok(_) => None,
            Err(problem) => {
                list.failed();
                Some(problem)
            }
        };
        if let Some(problem) = problem {
            crate::errln(format_args!("tmls-rs: {problem}"));
            self.say(Ok(problem));
        }
        self.invalidate();
    }

    /// A row of the list, clicked or under the cursor at Enter: its picture opens as one
    /// clicked in the terminal does, in a panel that keeps the list to go back to.
    fn open_made(&mut self, path: String) {
        let listed = self.side.as_ref().and_then(SidePanel::made_body);
        let Some(key) = listed.map(|body| body.key.clone()) else {
            return;
        };
        self.say(Ok(format!("opening {path} on {} …", key.host)));
        let link = FileLink { path, line: None };
        let click = self.view(move |local, cache, wanted| {
            viewer::open_link_while(&key, &link, local, cache, &viewer::run, wanted)
        });
        self.back_for = Some(click);
    }

    /// `‹`, Backspace or Esc at a picture opened from the list: the list is back as it was,
    /// and is asked for again (for the tab in front now, if that changed). With no tab left
    /// there is no session to list: the panel closes.
    fn side_back(&mut self) {
        if self.tabs.active().is_none() {
            return self.close_side();
        }
        let Some(body) = self.side.as_mut().and_then(|side| side.back.take()) else {
            return;
        };
        self.picture.set(&self.gpu.device, &self.gpu.queue, None);
        self.side = Some(SidePanel::made(body));
        self.show_made();
    }

    /// A key for the panel while it has the keys.
    fn panel_key(&mut self, key: PanelKey) {
        let lines = self.layout.side_lines;
        let from_list = self.side.as_ref().is_some_and(|side| side.back.is_some());
        let list = self.side.as_mut().and_then(SidePanel::made_body_mut);
        match (key, list) {
            (PanelKey::Up, Some(list)) => {
                if list.move_by(-1, lines) {
                    self.invalidate();
                }
            }
            (PanelKey::Down, Some(list)) => {
                if list.move_by(1, lines) {
                    self.invalidate();
                }
            }
            (PanelKey::Open, Some(list)) => {
                if let Some(path) = list.selected().map(|file| file.path.clone()) {
                    self.open_made(path);
                }
            }
            (PanelKey::Back, None) if from_list => self.side_back(),
            // From the list, back to the terminal.
            (PanelKey::Back, _) => self.leave_panel(),
            (PanelKey::Up | PanelKey::Down | PanelKey::Open, None) => {}
        }
    }

    /// The keys leave the panel: to the terminal, or to the list with no tab open.
    fn leave_panel(&mut self) {
        let to = match self.tabs.is_empty() {
            true => Focus::Sidebar,
            false => Focus::Terminal,
        };
        self.set_focus(to);
    }

    /// Whether the panel can have the keys: what it shows takes some, and the layout has a
    /// panel (a window too narrow for one draws none, though it is still open).
    fn panel_takes_keys(&self) -> bool {
        self.side.as_ref().is_some_and(SidePanel::takes_keys) && self.layout.side.w > 0.0
    }

    /// After the panel changed or closed, or the window was laid out again: if it had the
    /// keys and what it shows now takes none (a file from the terminal, nothing) or it is not
    /// drawn any more, they go back to the terminal (`keys::focus_after_panel`).
    fn settle_panel_focus(&mut self) {
        if self.modal.is_some() {
            // `close_modal` finds the panel has no keys to give back.
            return;
        }
        let takes = self.side.as_ref().is_some_and(SidePanel::takes_keys);
        let drawn = self.layout.side.w > 0.0;
        let to = keys::focus_after_panel(self.focus, takes, drawn, !self.tabs.is_empty());
        if to != self.focus {
            self.set_focus(to);
        }
    }

    /// What the panel's header has now, for `Layout::hit_with`.
    fn side_look(&self) -> SideLook {
        self.side.as_ref().map(SidePanel::look).unwrap_or_default()
    }

    /// What is under the pointer.
    fn hit(&self) -> Hit {
        let (x, y) = self.pointer;
        self.layout
            .hit_with(x, y, self.tabs.len(), self.side_look())
    }

    /// `Open in app` in the panel's header: the panel's file goes to the desktop's app.
    fn open_side_in_app(&mut self) {
        let Some((host, path)) = self
            .side
            .as_ref()
            .map(|side| (side.host.clone(), side.path.clone()))
        else {
            return;
        };
        self.say(Ok(format!("opening {path} in its app …")));
        self.view(move |local, cache, _| {
            viewer::open_in_app(&host, &path, local, cache, &viewer::run)
        });
    }

    /// Runs `work` (this machine's name, the cache directory, whether its answer is still
    /// waited for) on a thread of its own and delivers what it gives as the answer to a new
    /// click. Work that a newer click overtook can stop early: its answer is dropped anyway.
    /// Work that panics answers as a broken picture. Returns the click's number.
    fn view(
        &mut self,
        work: impl FnOnce(&str, Option<&std::path::Path>, &dyn Fn() -> bool) -> Result<Opened, String>
        + Send
        + 'static,
    ) -> u64 {
        let click = self.views.fetch_add(1, Ordering::SeqCst) + 1;
        self.back_for = None;
        let (proxy, local) = (self.proxy.clone(), self.cfg.local_name.clone());
        let (cache, views) = (self.view_cache.clone(), self.views.clone());
        std::thread::spawn(move || {
            let wanted = || views.load(Ordering::SeqCst) == click;
            let result = viewer::guarded(|| work(&local, cache.as_deref(), &wanted));
            let _ = proxy.send_event(UserEvent::Viewed(click, result));
        });
        click
    }

    /// No answer is waited for any more, to a click or for a list: one still on its way is
    /// dropped when it comes.
    fn cancel_view(&mut self) {
        self.views.fetch_add(1, Ordering::SeqCst);
        self.lists.fetch_add(1, Ordering::SeqCst);
        self.back_for = None;
    }

    /// Puts `panel` in the file panel, in place of what it showed, and lays the window out
    /// for it. Returns whether the window has room for one: beside `PANEL_MIN_COLS` columns of
    /// terminal there may be none, and then none is kept (the layout is the one without).
    fn show_side(&mut self, panel: SidePanel) -> bool {
        let was_open = self.side.is_some();
        self.side = Some(panel);
        if !was_open {
            self.relayout();
        }
        if self.layout.side.w <= 0.0 {
            self.side = None;
        }
        self.settle_panel_focus();
        self.side.is_some()
    }

    /// The answer to a click on a file: text or a picture opens in the panel (in place of
    /// what it showed) if the window has room for one, anything else is started in the
    /// desktop's app. An answer to a click that is not the latest is dropped (tmls: a newer
    /// click cancels the load). What went wrong shows in the status line until the next key.
    fn viewed(&mut self, click: u64, result: Result<Opened, String>) {
        if click != self.views.load(Ordering::SeqCst) {
            return;
        }
        let from_list = self.back_for.take() == Some(click);
        match result {
            Ok(Opened::Text(document)) => {
                let said = match &document.line {
                    Some(line) => format!("opened {} at line {line}", document.path),
                    None => format!("opened {}", document.path),
                };
                // Whatever the panel showed, it shows no picture now: its texture is freed.
                self.picture.set(&self.gpu.device, &self.gpu.queue, None);
                // No room: the click is answered. A note like the other answers here, gone
                // with the next key: nothing is broken.
                if !self.show_side(SidePanel::text(&document, &self.cfg.local_name)) {
                    return self.say(Ok(viewer::TOO_NARROW.to_string()));
                }
                let lines = self.layout.side_lines;
                if let Some(side) = &mut self.side {
                    side.reveal(lines);
                }
                self.say(Ok(said));
            }
            Ok(Opened::Image {
                host,
                path,
                file,
                picture,
            }) => {
                let size = (picture.width, picture.height);
                let mut panel = SidePanel::image(&host, &path, size, file, &self.cfg.local_name);
                // Opened from the list that is still there: the way back to it.
                let list = self.side.as_ref().and_then(SidePanel::made_body);
                panel.back = list.filter(|_| from_list).cloned();
                let said = match self.show_side(panel) {
                    // Uploaded once, in place of the picture before. The pixels are not kept
                    // here: `picture` is dropped when this arm ends.
                    true => {
                        match self
                            .picture
                            .set(&self.gpu.device, &self.gpu.queue, Some(&picture))
                        {
                            true => format!("opened {path}"),
                            false => {
                                self.close_side();
                                format!("{path}: too large a picture for this GPU")
                            }
                        }
                    }
                    false => {
                        self.picture.set(&self.gpu.device, &self.gpu.queue, None);
                        viewer::TOO_NARROW.to_string()
                    }
                };
                self.say(Ok(said));
            }
            Ok(Opened::Outside { target, said }) => {
                let said = links::open_path(&target).map(|()| said);
                self.say(Ok(said.unwrap_or_else(|problem| problem)));
            }
            // A list is not a click's answer: it comes as `UserEvent::Listed`.
            Ok(Opened::Made { .. }) => {}
            Err(problem) => {
                crate::errln(format_args!("tmls-rs: {problem}"));
                self.say(Ok(problem));
            }
        }
    }

    /// Closes the file panel: the terminal has its full width again.
    fn close_side(&mut self) {
        // An answer still on its way would open it again.
        self.cancel_view();
        // A picture's texture is freed.
        self.picture.set(&self.gpu.device, &self.gpu.queue, None);
        if self.side.take().is_some() {
            self.relayout();
        }
        self.settle_panel_focus();
    }

    /// Copies the command that attaches `key`.
    fn copy_attach(&mut self, key: &SessionKey) {
        if key.host == KITTY {
            return self.say(Err(format!(
                "{} runs outside tmux: nothing attaches it",
                key.name
            )));
        }
        let local = self.cfg.local_name.clone();
        let said = match self.clipboard() {
            Some(clipboard) => forms::copy_attach(key, &local, &mut |text| {
                clipboard.set_text(text).map_err(|e| e.to_string())
            }),
            None => Err("copy failed: no clipboard".to_string()),
        };
        self.say(said);
    }

    /// Ctrl+Shift+V in a form: the clipboard's text goes into the focused field.
    fn paste_into_form(&mut self) {
        let text = match self.clipboard().map(|clipboard| clipboard.get_text()) {
            Some(Ok(text)) => text,
            _ => return,
        };
        if let Some((Modal::Form(form), _)) = &mut self.modal {
            form.paste(&text);
            self.lookup_name();
            self.invalidate();
        }
    }

    /// A mouse button pressed while a form or menu is open: it is theirs, wherever it lands.
    fn modal_click(&mut self, button: MouseButton) {
        let (x, y) = self.pointer;
        let Some((modal, at)) = &mut self.modal else {
            return;
        };
        match modal {
            Modal::Menu(menu) => {
                let panel = menu.panel();
                let rect = self.layout.panel_rect(panel.cols, panel.lines, *at);
                let (cx, cy) = self.layout.panel_cell(rect, x, y);
                let result = match button {
                    MouseButton::Left => menu.click(cx, cy),
                    _ if panel.contains(cx, cy) => MenuResult::Pending,
                    _ => MenuResult::Closed,
                };
                self.menu_result(result);
            }
            Modal::Form(form) if button == MouseButton::Left => {
                let panel = form.panel();
                let rect = self.layout.panel_rect(panel.cols, panel.lines, *at);
                let (cx, cy) = self.layout.panel_cell(rect, x, y);
                let result = form.click(cx, cy);
                self.form_result(result);
            }
            Modal::Form(_) => {}
            Modal::Alerts(alerts) => {
                let panel = alerts.panel();
                let rect = self.layout.panel_rect(panel.cols, panel.lines, *at);
                let (cx, cy) = self.layout.panel_cell(rect, x, y);
                let result = match button {
                    MouseButton::Left => alerts.click(cx, cy),
                    _ if panel.contains(cx, cy) => PanelResult::Pending,
                    _ => PanelResult::Closed,
                };
                self.alerts_result(result);
            }
        }
        self.invalidate();
    }

    /// One of the window's own bindings (`Quit` is the event loop's).
    fn global(&mut self, global: Global) {
        match global {
            Global::NextTab => self.switch_tab(1),
            Global::PrevTab => self.switch_tab(-1),
            Global::FocusSidebar => self.set_focus(Focus::Sidebar),
            Global::FocusTerminal => {
                // With no tab open there is no terminal to give the keys to.
                if !self.tabs.is_empty() {
                    self.set_focus(Focus::Terminal);
                }
            }
            Global::CloseTab => {
                if let Some(active) = self.tabs.active() {
                    self.close_tab(active);
                }
            }
            Global::ClosePanel => self.close_side(),
            Global::ToggleFiles => self.toggle_files(),
            Global::NextWaiting => self.next_waiting(),
            Global::ZoomIn => self.zoom(ZoomDir::In),
            Global::ZoomOut => self.zoom(ZoomDir::Out),
            Global::Reattach => {
                if let Some(active) = self.tabs.active() {
                    let result = self.tabs.reattach(active);
                    self.report(result);
                    self.tab_changed();
                }
            }
            Global::Quit => {}
        }
    }

    /// Opens the next session after the front tab's that needs the user.
    /// Alt+Shift+Right / Left, as in tmls: among the pinned (★) sessions when any is listed —
    /// one without a tab gets one — else among the open tabs, and with fewer than two of
    /// those among all the sessions.
    fn switch_tab(&mut self, step: i32) {
        let front = self.tabs.active().map(|index| self.tabs.key(index));
        let pinned = self.sidebar.pin_step(front, step);
        let few_tabs = || match self.tabs.len() {
            0 | 1 => self.sidebar.list_step(front, step),
            _ => None,
        };
        if let Some(key) = pinned.or_else(few_tabs) {
            self.open(&key);
            return;
        }
        if step > 0 {
            self.tabs.next();
        } else {
            self.tabs.prev();
        }
        self.tab_changed();
    }

    fn next_waiting(&mut self) {
        let front = self.tabs.active().map(|index| self.tabs.key(index));
        if let Some(key) = self.sidebar.next_waiting(front) {
            self.open(&key);
        }
    }

    fn list_key(&mut self, key: ListKey) {
        self.select_when_listed = None;
        match key {
            ListKey::Up => self.sidebar.move_by(-1),
            ListKey::Down => self.sidebar.move_by(1),
            ListKey::Open => {
                if let Some(action) = self.sidebar.activate() {
                    self.act(action);
                }
            }
            ListKey::Pin => self.toggle_pin(),
            ListKey::NextWaiting => self.next_waiting(),
            ListKey::New => self.foot_button(Button::New),
            ListKey::Alerts => self.open_alerts(),
            ListKey::Menu => {
                // A session outside tmux has no menu: nothing in it applies.
                let selected = self.sidebar.selected().filter(|key| key.host != KITTY);
                if let Some(key) = selected.cloned() {
                    self.open_modal(Modal::Menu(RowMenu::new(key)), None);
                }
            }
        }
        self.invalidate();
    }

    /// Does what the list asked for.
    fn act(&mut self, action: sidebar::Action) {
        match action {
            sidebar::Action::Attach(key) => self.open(&key),
            sidebar::Action::NewSessionOn(host) => self.new_session_form(&host),
            sidebar::Action::Menu(key) => {
                let (x, y) = self.pointer;
                let at = (x.max(0.0) as u32, y.max(0.0) as u32);
                self.open_modal(Modal::Menu(RowMenu::new(key)), Some(at));
            }
        }
    }

    /// A mouse button pressed on the session list. Left: attach the session, or the `+` of a
    /// host's header. Middle: pin. Right: the session's menu.
    fn list_click(&mut self, button: MouseButton, line: usize, col: usize) {
        let Some(row) = self.sidebar.row_at(line, self.layout.list_lines) else {
            return;
        };
        self.select_when_listed = None;
        let header = matches!(self.sidebar.rows()[row], sidebar::Row::Host { .. });
        let action = match button {
            MouseButton::Left if header && !self.layout.on_add(col) => None,
            MouseButton::Left => self.sidebar.click(row),
            MouseButton::Right => self.sidebar.menu(row),
            MouseButton::Middle => {
                if let Some(key) = self.sidebar.key_at(row).cloned() {
                    self.sidebar.select_key(&key);
                    self.toggle_pin();
                }
                None
            }
            _ => None,
        };
        if let Some(action) = action {
            self.act(action);
        }
        self.invalidate();
    }

    /// A button at the foot of the sidebar; they act on the selected session.
    fn foot_button(&mut self, button: Button) {
        let selected = self.sidebar.selected().cloned();
        match (button, selected) {
            (Button::Alerts, _) => self.open_alerts(),
            (Button::Quit, _) => self.quit = true,
            (Button::Sketch, _) => {
                // Without a hub the button is not drawn and its place does nothing.
                if let Some(url) = self.sketchpad.clone() {
                    self.say(links::open(&url).map(|()| format!("opened {url}")));
                }
            }
            (Button::Open, Some(key)) => self.open(&key),
            (Button::Copy, Some(key)) => self.copy_attach(&key),
            (Button::New, selected) => {
                // From a session outside tmux: on this machine.
                let host = selected.map(|key| match key.host == KITTY {
                    true => self.cfg.local_name.clone(),
                    false => key.host,
                });
                if let Some(host) = host.or_else(|| self.cfg.hosts.first().cloned()) {
                    self.act(sidebar::Action::NewSessionOn(host));
                }
            }
            // The keys' panel is not built.
            (Button::Help, _) | (Button::Open | Button::Copy, None) => {}
        }
    }

    /// A Claude session outside tmux has no tab: its kitty window is brought to the front
    /// (`kitty::focus`, on a thread of its own; the status line says how it went).
    fn focus_outside(&mut self, key: &SessionKey) {
        let Some(pid) = self.sidebar.pid(key) else {
            return self.say(Err(format!("{} is not listed any more", key.name)));
        };
        // Looked at: its ◆ goes, as a tab's does when it comes to the front.
        if self.sidebar.opened(&mut self.seen, key) {
            self.invalidate();
        }
        let (proxy, name) = (self.proxy.clone(), key.name.clone());
        std::thread::spawn(move || {
            let said = match kitty::focus(pid) {
                Ok(()) => Ok(format!("{name}: its window is in front")),
                Err(e) => Err(format!("{name} runs outside tmux; {e}")),
            };
            let _ = proxy.send_event(UserEvent::Said(said));
        });
    }

    /// Opens the session's tab (or brings it to the front) and gives it the keys.
    fn open(&mut self, key: &SessionKey) {
        if key.host == KITTY {
            return self.focus_outside(key);
        }
        let result = self.tabs.open(key, self.layout.grid);
        let opened = result.is_ok();
        self.report(result.map(|_| ()));
        if opened {
            self.tab_changed();
        }
    }

    /// Pins or unpins the selected session; a `pins` file that cannot be written shows in the
    /// status line.
    fn toggle_pin(&mut self) {
        self.sidebar.toggle_pin();
        let saved = self.sidebar.take_pin_error();
        self.report(saved);
    }

    /// Keeps what went wrong for the status line, or clears it.
    fn report(&mut self, result: anyhow::Result<()>) {
        self.problem = result.err().map(|e| {
            crate::errln(format_args!("tmls-rs: {e:#}"));
            format!("{e:#}")
        });
        self.invalidate();
    }

    /// Closes a tab. The keys move only when that changes which tab is in front: closing one
    /// in the back while the list has the keys leaves them with the list.
    fn close_tab(&mut self, index: usize) {
        let front = self
            .tabs
            .active()
            .map(|active| self.tabs.key(active).clone());
        self.tabs.close(index);
        // One client fewer on that session: the row's count is read again now.
        self.poller.refresh_now();
        let now = self.tabs.active().map(|active| self.tabs.key(active));
        let front_changed = now != front.as_ref();
        let focus = keys::focus_after_close(self.focus, front_changed, self.tabs.is_empty());
        if front_changed {
            self.tab_changed();
        }
        if self.modal.is_some() {
            // The open form keeps the keys; they go there when it closes.
            self.focus_before = focus;
        } else if focus != self.focus {
            self.set_focus(focus);
        }
        self.invalidate();
    }

    /// Another tab is in front, or the one in front has a new session: the keys go to the
    /// terminal, and a key or a mouse button still held is not this tab's. The list's cursor
    /// goes to the tab's session, which counts as seen from now on; the window is named after
    /// it.
    fn tab_changed(&mut self) {
        self.gate.hold();
        self.ime_at = None;
        // A button held over the tab that was in front is not this tab's (the selection that
        // was being dragged there ended in `Tabs::select`).
        self.mouse.reset();
        let front = self.tabs.active().map(|active| self.tabs.key(active));
        match front {
            Some(key) => {
                self.sidebar.select_key(key);
                self.sidebar.opened(&mut self.seen, key);
                // tmls: `session · machine` (its `set_title`).
                self.window
                    .set_title(&format!("{} · {}", key.name, key.host));
            }
            None => self.window.set_title(TITLE),
        }
        let no_tab = front.is_none();
        if self.modal.is_some() {
            self.focus_before = Focus::Terminal;
        } else if !self.tabs.is_empty() {
            self.set_focus(Focus::Terminal);
        }
        // The panel's list is of the session in front: of this one now.
        if self.side.as_ref().is_some_and(|s| s.made_body().is_some()) {
            match no_tab {
                true => self.close_side(),
                false => self.show_made(),
            }
        }
        self.invalidate();
    }

    fn set_focus(&mut self, focus: Focus) {
        if focus == Focus::Terminal {
            self.gate.hold();
        } else if let Some(session) = self.tabs.live_mut() {
            // A composition in the terminal ends when it loses the keys.
            session.set_preedit("");
        }
        self.focus = focus;
        self.sync_focus();
        self.invalidate();
    }

    /// After anything that may have moved the keyboard focus: the front tab hears whether it
    /// has it (the window has it, and the terminal inside the window).
    fn sync_focus(&mut self) {
        let terminal = self.focus == Focus::Terminal;
        self.tabs.set_focused(self.window_focused && terminal);
        // The list takes keys, not text: `j` / `k` must not start a composition.
        if terminal != self.ime_allowed {
            self.ime_allowed = terminal;
            self.window.set_ime_allowed(terminal);
        }
    }

    fn ime(&mut self, t_key_ns: u128, ime: &Ime) {
        if self.focus != Focus::Terminal {
            return;
        }
        let Some(session) = self.tabs.live_mut() else {
            return;
        };
        let changed = match ime {
            Ime::Preedit(text, _) => session.set_preedit(text),
            Ime::Commit(text) => {
                session.ime_commit(t_key_ns, text);
                true
            }
            Ime::Disabled => session.set_preedit(""),
            Ime::Enabled => false,
        };
        if changed {
            self.invalidate();
        }
    }

    fn clipboard(&mut self) -> Option<&mut arboard::Clipboard> {
        if self.clipboard.is_none() {
            match arboard::Clipboard::new() {
                Ok(clipboard) => self.clipboard = Some(clipboard),
                Err(e) => crate::errln(format_args!("tmls-rs: no clipboard: {e}")),
            }
        }
        self.clipboard.as_mut()
    }

    /// Ctrl+Shift+C: copies the front tab's selection.
    fn copy(&mut self) {
        let Some(text) = self
            .tabs
            .active()
            .and_then(|index| self.tabs.session(index).selected_text())
        else {
            return;
        };
        self.set_clipboard(text);
    }

    fn set_clipboard(&mut self, text: String) {
        if let Some(clipboard) = self.clipboard()
            && let Err(e) = clipboard.set_text(text)
        {
            crate::errln(format_args!("tmls-rs: copy failed: {e}"));
        }
    }

    /// The modifiers held, for a mouse event.
    fn mouse_mods(&self) -> s2_core::Mods {
        s2_core::Mods {
            shift: self.mods.shift_key(),
            ctrl: self.mods.control_key(),
            alt: self.mods.alt_key(),
            logo: self.mods.super_key(),
        }
    }

    /// The cell of the front tab nearest to the pointer.
    fn pointer_cell(&self) -> Option<(u16, u16)> {
        let (x, y) = self.pointer;
        self.tabs.front_scene()?.nearest_cell(x, y)
    }

    /// Does what `mouse::Pointer` decided, to the tab in front.
    fn mouse_acts(&mut self, acts: Vec<mouse::Act>) {
        let Some(active) = self.tabs.active() else {
            return;
        };
        for act in acts {
            match mouse::perform(act, self.tabs.session_mut(active)) {
                Some(Effect::Redraw) => self.invalidate(),
                Some(Effect::Copy(text)) => self.set_clipboard(text),
                Some(Effect::Open(url)) => {
                    let said = links::open(&url).map(|()| format!("opened {url}"));
                    self.say(said);
                }
                Some(Effect::File(link)) => self.open_file(link),
                None => {}
            }
        }
    }

    /// Ctrl+Shift+V: pastes the clipboard into the front tab, if its session is live.
    fn paste(&mut self) {
        let text = match self.clipboard().map(|clipboard| clipboard.get_text()) {
            Some(Ok(text)) => text,
            Some(Err(e)) => return crate::errln(format_args!("tmls-rs: paste failed: {e}")),
            None => return,
        };
        if let Some(session) = self.tabs.live_mut() {
            session.paste(&text);
        }
    }

    fn pointer_moved(&mut self, position: PhysicalPosition<f64>) {
        self.pointer = (position.x as f32, position.y as f32);
        if let Some((Modal::Menu(menu), at)) = &mut self.modal {
            let panel = menu.panel();
            let rect = self.layout.panel_rect(panel.cols, panel.lines, *at);
            let (x, y) = self.layout.panel_cell(rect, self.pointer.0, self.pointer.1);
            if menu.hover(x, y) {
                self.invalidate();
            }
        }
        if let Some((Modal::Alerts(alerts), at)) = &mut self.modal {
            let panel = alerts.panel();
            let rect = self.layout.panel_rect(panel.cols, panel.lines, *at);
            let (x, y) = self.layout.panel_cell(rect, self.pointer.0, self.pointer.1);
            if alerts.hover(x, y) {
                self.invalidate();
            }
        }
        let (x, y) = self.pointer;
        let hit = self.layout.hit(x, y, self.tabs.len());
        if self.dragging_separator {
            self.drag_separator(x);
        }
        if let Some(offset) = self.dragging_divider {
            self.drag_divider(x, offset);
        }
        let dragging = self.dragging_separator || self.dragging_divider.is_some();
        let on_handle = matches!(hit, Hit::Separator | Hit::SideDivider);
        let resize = dragging || (self.modal.is_none() && on_handle);
        if resize != self.resize_cursor {
            self.resize_cursor = resize;
            self.window.set_cursor(match resize {
                true => CursorIcon::ColResize,
                false => CursorIcon::Default,
            });
        }
        if dragging {
            return;
        }
        let over_terminal = self.modal.is_none() && hit == Hit::Terminal;
        if !(self.mouse.held() || over_terminal) {
            return;
        }
        if let Some(at) = self.pointer_cell() {
            let reporting = self.tabs.mouse_reporting();
            let acts = self.mouse.motion(at, self.mouse_mods(), reporting);
            self.mouse_acts(acts);
        }
    }

    fn button(&mut self, state: ElementState, button: MouseButton) {
        let (x, y) = self.pointer;
        let terminal_button = match button {
            MouseButton::Left => Some(s2_core::MouseButton::Left),
            MouseButton::Middle => Some(s2_core::MouseButton::Middle),
            MouseButton::Right => Some(s2_core::MouseButton::Right),
            _ => None,
        };
        if state == ElementState::Released {
            let separator = std::mem::take(&mut self.dragging_separator);
            if separator || self.dragging_divider.take().is_some() {
                return self.save_widths();
            }
            if let Some(button) = terminal_button {
                let acts = self
                    .mouse
                    .release(button, self.pointer_cell(), self.mouse_mods());
                self.mouse_acts(acts);
            }
            return;
        }
        if self.modal.is_some() {
            return self.modal_click(button);
        }
        let hit = self.hit();
        if let Hit::List { line, col } = hit {
            return self.list_click(button, line, col);
        }
        if hit == Hit::Terminal
            && let Some(button) = terminal_button
            && self.tabs.active().is_some()
        {
            self.set_focus(Focus::Terminal);
            let reporting = self.tabs.mouse_reporting();
            let acts = self
                .mouse
                .press(button, self.pointer_cell(), self.mouse_mods(), reporting);
            return self.mouse_acts(acts);
        }
        if button != MouseButton::Left {
            return;
        }
        match hit {
            Hit::Separator => self.dragging_separator = true,
            Hit::SideDivider => self.dragging_divider = Some(x - self.layout.side.x),
            Hit::Button(button) => self.foot_button(button),
            Hit::Tab(index) => {
                self.tabs.select(index);
                self.tab_changed();
            }
            Hit::TabClose(index) => self.close_tab(index),
            Hit::Files => self.toggle_files(),
            Hit::SideClose => self.close_side(),
            Hit::SideOpen => {
                self.focus_panel();
                self.open_side_in_app();
            }
            Hit::SideRefresh => {
                self.focus_panel();
                self.show_made();
            }
            Hit::SideBack => {
                self.focus_panel();
                self.side_back();
            }
            // A picture: between fitted and its own size. A row of the list: its picture.
            // Text has nothing to click.
            Hit::Side => {
                self.focus_panel();
                let body = self.layout.side_body;
                if !body.contains(x, y) {
                    return;
                }
                let lines = self.layout.side_lines;
                let line = ((y - body.y) / self.layout.cell.1.max(1.0)).floor() as usize;
                if let Some(list) = self.side.as_mut().and_then(SidePanel::made_body_mut) {
                    let path = list.click(line, lines).map(|file| file.path.clone());
                    self.invalidate();
                    if let Some(path) = path {
                        self.open_made(path);
                    }
                } else if self.side.as_mut().is_some_and(SidePanel::click) {
                    self.invalidate();
                }
            }
            Hit::List { .. }
            | Hit::Sidebar
            | Hit::TabBar
            | Hit::Terminal
            | Hit::Status
            | Hit::Outside => {}
        }
    }

    /// A click on the panel gives it the keys, if what it shows takes any.
    fn focus_panel(&mut self) {
        let takes = self.panel_takes_keys();
        if takes && self.focus != Focus::Panel {
            self.set_focus(Focus::Panel);
        }
    }

    /// The wheel scrolls what the pointer is over: the front tab (its program's history or the
    /// scrollback here, `mouse::Pointer::wheel`), the file panel, or the session list.
    fn wheel(&mut self, delta: &MouseScrollDelta) {
        let cell_height = self.atlas.metrics().height;
        let rows = input::wheel_rows(delta, cell_height, &mut self.wheel_carry);
        if self.modal.is_some() {
            return;
        }
        match self.hit() {
            Hit::Terminal => {
                // To the program if it takes the mouse (tmux scrolls its history), else the
                // viewport moves; the frame at the new position comes back through the waker.
                let reporting = self.tabs.mouse_reporting();
                let acts =
                    self.mouse
                        .wheel(rows, self.pointer_cell(), self.mouse_mods(), reporting);
                self.mouse_acts(acts);
            }
            Hit::List { .. } | Hit::Button(_) | Hit::Sidebar if rows != 0 => {
                self.sidebar.scroll(rows);
                self.invalidate();
            }
            Hit::Side
            | Hit::SideDivider
            | Hit::SideOpen
            | Hit::SideRefresh
            | Hit::SideBack
            | Hit::SideClose => {
                let (lines, body) = (self.layout.side_lines, self.layout.side_body);
                // A picture at its own size moves by as many pixels as text by lines, and
                // sideways with Shift; text moves by lines.
                let pixels = rows as f32 * cell_height;
                let by = match self.mods.shift_key() {
                    true => (pixels, 0.0),
                    false => (0.0, pixels),
                };
                if self
                    .side
                    .as_mut()
                    .is_some_and(|side| side.scroll(rows, lines) || side.pan(by, body))
                {
                    self.invalidate();
                }
            }
            _ => {}
        }
    }

    /// Takes the front tab's waiting frames and, if there is something new to show, draws and
    /// presents.
    fn redraw(&mut self, cause: Redraw) -> anyhow::Result<()> {
        if let Some(marks) = &mut self.marks {
            marks.begin_redraw();
        }
        // A tab whose command ended looks different in the tab bar, in front or not.
        if self.tabs.state_changes() {
            self.dirty = true;
            // A session ended (killed, exited, ssh dropped): the list hears of it now.
            self.poller.refresh_now();
        }
        let origin = (self.layout.terminal.x, self.layout.terminal.y);
        let new_frame = self
            .tabs
            .take_frames(origin, &mut self.atlas, &self.theme.terminal);
        if !new_frame && !self.dirty && cause == Redraw::Woken {
            return Ok(());
        }
        if self.dirty {
            // Something other than terminal output changed; that may have been the chrome. A
            // redraw for a new frame alone (a key echo) leaves its quads as they are.
            self.view = self.build_view();
            self.chrome.invalidate();
        }
        let front = self.tabs.front();
        // What the latency log and the IME need of the frame drawn.
        let drawn = front
            .as_ref()
            .map(|front| (front.frame.seq, front.frame.cursor));
        draw::compose(
            front,
            !new_frame,
            &mut self.atlas,
            &self.theme,
            &mut self.chrome,
            &self.view,
            &self.sidebar,
            &self.layout,
        );
        self.renderer.prepare(
            &self.gpu.device,
            &self.gpu.queue,
            &mut self.atlas,
            &draw::layer_order(
                self.tabs.front_scene().filter(|_| drawn.is_some()),
                &self.chrome,
            ),
            (self.config.width, self.config.height),
        );

        let (acquired, texture) = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => (Acquired::Ready, Some(texture)),
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => {
                (Acquired::Suboptimal, Some(texture))
            }
            wgpu::CurrentSurfaceTexture::Outdated => (Acquired::Outdated, None),
            wgpu::CurrentSurfaceTexture::Lost => (Acquired::Lost, None),
            wgpu::CurrentSurfaceTexture::Timeout => (Acquired::Timeout, None),
            wgpu::CurrentSurfaceTexture::Occluded => (Acquired::Occluded, None),
            wgpu::CurrentSurfaceTexture::Validation => (Acquired::Validation, None),
        };
        let (texture, suboptimal) = match (surface::after_acquire(acquired), texture) {
            (Next::Present { reconfigure }, Some(texture)) => (texture, reconfigure),
            // The frames taken above are not on screen (their keys stay in the log until a
            // frame is presented): this redraw has to happen again.
            (Next::Retry { reconfigure, after }, _) => {
                if reconfigure {
                    self.surface.configure(&self.gpu.device, &self.config);
                }
                if after.is_zero() {
                    self.invalidate();
                } else {
                    self.dirty = true;
                    self.retry_at = Some(Instant::now() + after);
                }
                return Ok(());
            }
            (Next::Fail, _) | (Next::Present { .. }, None) => {
                bail!("the surface rejected the frame")
            }
        };
        let view = texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("tmls-rs frame"),
            });
        self.renderer
            .draw(&mut encoder, &view, self.theme.terminal.bg);
        // A picture in the file panel: a second pass over the first one's body, where
        // `place` puts it in the body as it is for this frame. Without one, nothing.
        if let Some(image) = self.side.as_ref().and_then(SidePanel::image_body) {
            let body = self.layout.side_body;
            let (dest, source) = viewer::place(image.size, body, image.actual, image.scroll);
            // The pass draws over everything, a form or a menu too: not while one is on it.
            let covered = self.view.panel.as_ref().is_some_and(|(panel, at)| {
                let over = self.layout.panel_rect(panel.cols, panel.lines, *at);
                let line = self.layout.separator.w.max(1.0);
                over.x - line < dest.right()
                    && dest.x < over.right() + line
                    && over.y - line < dest.bottom()
                    && dest.y < over.bottom() + line
            });
            if !covered {
                let viewport = (self.config.width, self.config.height);
                self.picture
                    .draw(&mut encoder, &view, viewport, dest, source, body);
            }
        }
        drop(view);

        self.write_meta(false);
        let mut path = SurfacePath {
            queue: &self.gpu.queue,
            window: &self.window,
            commands: Some(encoder.finish()),
            texture: Some(texture),
        };
        // A `frame` line only for a frame of the tab that writes the log.
        let logged_seq = self
            .tabs
            .active()
            .filter(|&active| self.tabs.logged(active))
            .and(drawn)
            .map(|(seq, _)| seq);
        match (&mut self.marks, logged_seq) {
            (Some(marks), Some(seq)) => marks.present_frame(seq, &mut path),
            _ => {
                path.submit(WorkDone::default());
                path.present();
            }
        }
        self.tabs.presented();
        self.dirty = false;
        self.settle_marks();
        if suboptimal {
            self.surface.configure(&self.gpu.device, &self.config);
        }
        let cursor = drawn.and_then(|(_, cursor)| cursor);
        self.place_ime(cursor.map(|cursor| (cursor.col, cursor.row)));
        Ok(())
    }

    /// Runs wgpu's callbacks and writes the `frame` lines that are complete. Returns whether a
    /// line still waits for its work-done time.
    fn settle_marks(&mut self) -> bool {
        let Some(marks) = &mut self.marks else {
            return false;
        };
        if marks.pending() == 0 {
            return false;
        }
        let _ = self.gpu.device.poll(wgpu::PollType::Poll);
        marks.settle() > 0
    }

    /// The monitor's refresh rate as far as winit knows it now (`meta::refresh` picks).
    fn refresh(&self) -> Refresh {
        let current = self
            .window
            .current_monitor()
            .map(|monitor| monitor.refresh_rate_millihertz());
        let available: Vec<Option<u32>> = self
            .window
            .available_monitors()
            .map(|monitor| monitor.refresh_rate_millihertz())
            .collect();
        meta::refresh(current, &available)
    }

    /// The `meta` line, written once: as in `s2-wgpu` (tried before every present and after
    /// every batch of events, from the first present on, until the refresh rate is known or
    /// `meta::META_WAIT` has passed), with `app: "tmls-rs"`.
    fn write_meta(&mut self, exiting: bool) {
        if self.meta.written {
            return;
        }
        let Some(log) = &self.log else {
            self.meta.written = true;
            return;
        };
        let refresh = self.refresh();
        let waited = self
            .meta
            .first_try
            .get_or_insert_with(Instant::now)
            .elapsed();
        if !meta::write_now(&refresh, waited, exiting) {
            return;
        }
        self.meta.written = true;
        let fixed = meta::Fixed {
            git: meta::GIT,
            profile: if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
            scale: self.window.scale_factor(),
            present_mode: "fifo",
            load: "quiet",
        };
        meta::write_as(log, "tmls-rs", &fixed, &refresh, true);
    }

    /// When `write_meta` has to be tried again at the latest; `None` once it is written or
    /// before the first present.
    fn meta_deadline(&self) -> Option<Instant> {
        match (self.meta.written, self.meta.first_try) {
            (false, Some(first)) => Some(first + meta::META_WAIT),
            _ => None,
        }
    }

    /// Keeps the input method's candidate window at the terminal's cursor cell while the
    /// terminal has the keys.
    fn place_ime(&mut self, cursor: Option<(u16, u16)>) {
        let (width, height) = self.layout.cell;
        let at = cursor
            .filter(|_| self.focus == Focus::Terminal)
            .map(|(col, row)| {
                let terminal = self.layout.terminal;
                (
                    terminal.x + col as f32 * width,
                    terminal.y + row as f32 * height,
                )
            });
        if at == self.ime_at {
            return;
        }
        self.ime_at = at;
        if let Some((x, y)) = at {
            self.window.set_ime_cursor_area(
                PhysicalPosition::new(x, y),
                PhysicalSize::new(width, height),
            );
        }
    }

    /// Writes what is still pending and flushes the log. Dropping `self` afterwards stops the
    /// polling and emulator threads and closes the PTYs, which detaches every tab.
    fn shutdown(mut self) {
        // Stops the sampler thread first, so its last line is in before the final flush.
        drop(self.sampler.take());
        self.write_meta(true);
        let _ = self.gpu.device.poll(wgpu::PollType::Poll);
        if let Some(marks) = &mut self.marks {
            marks.finish();
        }
        if let Some(log) = &self.log {
            log.flush();
        }
    }
}

/// The real `PresentPath`: `queue.submit` with the work-done callback, then `present`, paced
/// by the compositor (see the module comment).
struct SurfacePath<'a> {
    queue: &'a wgpu::Queue,
    window: &'a Window,
    commands: Option<wgpu::CommandBuffer>,
    texture: Option<wgpu::SurfaceTexture>,
}

impl PresentPath for SurfacePath<'_> {
    fn submit(&mut self, done: WorkDone) {
        self.queue.submit(self.commands.take());
        self.queue
            .on_submitted_work_done(move || done.mark(now_ns()));
    }

    fn present(&mut self) {
        self.window.pre_present_notify();
        if let Some(texture) = self.texture.take() {
            self.queue.present(texture);
        }
    }
}
