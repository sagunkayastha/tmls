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
use client_core::{Config, HostSnapshot, Poller, Seen, SessionKey, default_color};
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
use winit::window::{Window, WindowId};

use crate::alerts::{self, Alerts, AlertsPanel, PanelResult, Settings, Watching};
use crate::draw::{self, Button, Chrome, ENDED_NOTICE, Hit, Layout, TabView, View};
use crate::forms::{self, Form, FormResult, MenuItem, MenuResult, Request, RowMenu};
use crate::keys::{self, Focus, Global, ListKey, RepeatGate, Route};
use crate::links;
use crate::mouse::{self, Effect, Pointer};
use crate::sidebar::{self, Sidebar};
use crate::tabs::{TabState, Tabs};
use crate::theme::{Theme, host_rgb, mark_rgb};

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
                eprintln!("tmls-rs: focus {focused}");
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
            WindowEvent::MouseInput { state, button, .. } => running.button(state, button),
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

        let theme = Theme::default();
        let mut atlas = Atlas::new(theme.font.clone(), window.scale_factor() as f32);
        atlas.set_max_height(gpu.device.limits().max_texture_dimension_2d);
        let layout = draw::layout(
            (config.width, config.height),
            atlas.scale(),
            &atlas.metrics(),
            &theme,
        );
        let renderer = Renderer::new(&gpu.device, config.format);

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
        let poller = Poller::start(
            &cfg,
            POLL_PERIOD,
            POLL_TIMEOUT,
            Box::new(move |snapshot| {
                let _ = poll_proxy.send_event(UserEvent::Snapshot(snapshot));
            }),
        );

        let (cols, rows) = layout.grid;
        eprintln!(
            "tmls-rs: adapter {:?} / {} / {} {}; format {:?}; present fifo; grid {cols}x{rows}; hosts {:?}",
            info.backend, info.name, info.driver, info.driver_info, config.format, cfg.hosts,
        );
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
            eprintln!("tmls-rs: scale factor {scale}");
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

    /// Window size, scale factor or font size changed: lays the window out again and tells
    /// the tabs how many cells fit now.
    fn relayout(&mut self) {
        self.layout = draw::layout(
            (self.config.width, self.config.height),
            self.atlas.scale(),
            &self.atlas.metrics(),
            &self.theme,
        );
        self.tabs.resize_all(self.layout.grid);
        self.invalidate();
    }

    /// A host's new listing, in the order the poller delivers them. The session of the tab in
    /// front is being watched: it gets no "finished unwatched" mark.
    fn snapshot(&mut self, snapshot: HostSnapshot) {
        let front = self.tabs.active().map(|index| self.tabs.key(index));
        let (host, online) = (snapshot.host.clone(), snapshot.online);
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
        self.invalidate();
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
            unread: self.alerts.unread(),
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
        let back = match self.focus_before {
            Focus::Terminal if !self.tabs.is_empty() => Focus::Terminal,
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
                // A form or menu opened since then keeps the keys.
                if self.modal.is_none() {
                    self.open(&key);
                }
            }
            Request::Rename(old, name) => {
                if let Some(index) = self.tabs.index_of(&old) {
                    let front = self.tabs.active() == Some(index);
                    self.close_tab(index);
                    let key = SessionKey {
                        host: old.host,
                        name,
                    };
                    let back = self.tabs.active();
                    self.select_when_listed = Some(key.clone());
                    if self.modal.is_none() {
                        self.open(&key);
                        // A tab that was in the back stays there.
                        if let (false, Some(back)) = (front, back) {
                            self.tabs.select(back);
                            self.tab_changed();
                        }
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
                eprintln!("tmls-rs: {problem}");
                (self.note, self.problem) = (None, Some(problem));
            }
        }
        self.invalidate();
    }

    /// Copies the command that attaches `key`.
    fn copy_attach(&mut self, key: &SessionKey) {
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
            Global::NextTab => {
                self.tabs.next();
                self.tab_changed();
            }
            Global::PrevTab => {
                self.tabs.prev();
                self.tab_changed();
            }
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
                if let Some(key) = self.sidebar.selected().cloned() {
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
            (Button::Open, Some(key)) => self.open(&key),
            (Button::Copy, Some(key)) => self.copy_attach(&key),
            (Button::New, selected) => {
                let host = selected.map(|key| key.host);
                if let Some(host) = host.or_else(|| self.cfg.hosts.first().cloned()) {
                    self.act(sidebar::Action::NewSessionOn(host));
                }
            }
            // The keys' panel is not built.
            (Button::Help, _) | (Button::Open | Button::Copy, None) => {}
        }
    }

    /// Opens the session's tab (or brings it to the front) and gives it the keys.
    fn open(&mut self, key: &SessionKey) {
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
            eprintln!("tmls-rs: {e:#}");
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
    /// terminal, and a key still held is not typed into it. The list's cursor goes to the
    /// tab's session, which counts as seen from now on; the window is named after it.
    fn tab_changed(&mut self) {
        self.gate.hold();
        self.ime_at = None;
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
        if self.modal.is_some() {
            self.focus_before = Focus::Terminal;
        } else if !self.tabs.is_empty() {
            self.set_focus(Focus::Terminal);
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
                Err(e) => eprintln!("tmls-rs: no clipboard: {e}"),
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
            eprintln!("tmls-rs: copy failed: {e}");
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
                None => {}
            }
        }
    }

    /// Ctrl+Shift+V: pastes the clipboard into the front tab, if its session is live.
    fn paste(&mut self) {
        let text = match self.clipboard().map(|clipboard| clipboard.get_text()) {
            Some(Ok(text)) => text,
            Some(Err(e)) => return eprintln!("tmls-rs: paste failed: {e}"),
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
        let over_terminal =
            self.modal.is_none() && self.layout.hit(x, y, self.tabs.len()) == Hit::Terminal;
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
        let hit = self.layout.hit(x, y, self.tabs.len());
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
            Hit::Button(button) => self.foot_button(button),
            Hit::Tab(index) => {
                self.tabs.select(index);
                self.tab_changed();
            }
            Hit::TabClose(index) => self.close_tab(index),
            Hit::List { .. }
            | Hit::Sidebar
            | Hit::TabBar
            | Hit::Terminal
            | Hit::Status
            | Hit::Outside => {}
        }
    }

    /// The wheel scrolls what the pointer is over: the front tab (its program's history or the
    /// scrollback here, `mouse::Pointer::wheel`), or the session list.
    fn wheel(&mut self, delta: &MouseScrollDelta) {
        let cell_height = self.atlas.metrics().height;
        let rows = input::wheel_rows(delta, cell_height, &mut self.wheel_carry);
        let (x, y) = self.pointer;
        if self.modal.is_some() {
            return;
        }
        match self.layout.hit(x, y, self.tabs.len()) {
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
