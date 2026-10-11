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
//! - a key reaches a session only through `keys::route` → `Route::Terminal`, so keys typed
//!   while the list has the focus, or at a tab whose session ended, reach no PTY and no log.
//!
//! List path: a polling thread delivers a host's listing → `UserEvent::Snapshot` →
//! `Sidebar::deliver` (which marks what finished unwatched) → a redraw that rebuilds the chrome.
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
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow, bail};
use client_core::{Config, HostSnapshot, Poller, Seen, SessionKey, default_color, tmux};
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

use crate::draw::{self, Button, Chrome, ENDED_NOTICE, Hit, Layout, TabView, View};
use crate::keys::{self, Focus, Global, ListKey, RepeatGate, Route};
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
    /// The last thing that went wrong (a tab that could not be opened), for the status line.
    problem: Option<String>,
    /// Keeps the polling threads alive.
    _poller: Poller,

    log: Option<Log>,
    marks: Option<Marks<Log>>,
    sampler: Option<Sampler>,
    meta: Meta,
    clipboard: Option<arboard::Clipboard>,
    mods: ModifiersState,
    pointer: (f32, f32),
    wheel_carry: f32,
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
                let _ = proxy.send_event(UserEvent::Snapshot(snapshot));
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
            problem: None,
            _poller: poller,
            log,
            clipboard: None,
            mods: ModifiersState::empty(),
            pointer: (-1.0, -1.0),
            wheel_carry: 0.0,
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
        self.sidebar.deliver(&mut self.seen, snapshot, front);
        self.invalidate();
    }

    /// What `chrome` draws besides the session list, from the state as it is now.
    fn build_view(&self) -> View {
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
        let status = match (&self.problem, active) {
            (Some(problem), _) => problem.clone(),
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
        let ev = match action {
            Action::Send(ev) => ev,
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
        match keys::route(&ev, self.focus, self.tabs.active_state()) {
            Route::Global(Global::Quit) => return After::Quit,
            Route::Global(global) => self.global(global),
            Route::Terminal => {
                if self.gate.passes(&ev) {
                    self.tabs.send_key(t_key_ns, ev);
                }
            }
            Route::Sidebar => {
                if let Some(key) = keys::list_key(&ev) {
                    self.list_key(key);
                }
            }
            Route::Form | Route::Drop => {}
        }
        After::Stay
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
        match key {
            ListKey::Up => self.sidebar.move_by(-1),
            ListKey::Down => self.sidebar.move_by(1),
            ListKey::Open => {
                if let Some(action) = self.sidebar.activate() {
                    self.act(action);
                }
            }
            ListKey::Pin => {
                self.sidebar.toggle_pin();
            }
            ListKey::NextWaiting => self.next_waiting(),
        }
        self.invalidate();
    }

    /// Does what the list asked for.
    fn act(&mut self, action: sidebar::Action) {
        match action {
            sidebar::Action::Attach(key) => self.open(&key),
            // The new-session form and the session menu are not built yet (Tasks 5 and 6 of
            // the plan); the list already asks for them.
            sidebar::Action::NewSessionOn(_) | sidebar::Action::Menu(_) => {}
        }
    }

    /// A mouse button pressed on the session list. Left: attach the session, or the `+` of a
    /// host's header. Middle: pin. Right: the session's menu.
    fn list_click(&mut self, button: MouseButton, line: usize, col: usize) {
        let Some(row) = self.sidebar.row_at(line, self.layout.list_lines) else {
            return;
        };
        let header = matches!(self.sidebar.rows()[row], sidebar::Row::Host { .. });
        let action = match button {
            MouseButton::Left if header && !self.layout.on_add(col) => None,
            MouseButton::Left => self.sidebar.click(row),
            MouseButton::Right => self.sidebar.menu(row),
            MouseButton::Middle => {
                if let Some(key) = self.sidebar.key_at(row).cloned() {
                    self.sidebar.select_key(&key);
                    self.sidebar.toggle_pin();
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
            (Button::Open, Some(key)) => self.open(&key),
            (Button::Copy, Some(key)) => {
                let text = tmux::attach_text(&key, &self.cfg.local_name);
                if let Some(clipboard) = self.clipboard()
                    && let Err(e) = clipboard.set_text(text)
                {
                    eprintln!("tmls-rs: copy failed: {e}");
                }
            }
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
        let now = self.tabs.active().map(|active| self.tabs.key(active));
        let front_changed = now != front.as_ref();
        let focus = keys::focus_after_close(self.focus, front_changed, self.tabs.is_empty());
        if front_changed {
            self.tab_changed();
        }
        if focus != self.focus {
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
        if !self.tabs.is_empty() {
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
        if let Some(clipboard) = self.clipboard()
            && let Err(e) = clipboard.set_text(text)
        {
            eprintln!("tmls-rs: copy failed: {e}");
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
        let Some(active) = self.tabs.active() else {
            return;
        };
        if !self.tabs.session(active).selecting() {
            return;
        }
        let cell = self
            .tabs
            .front_scene()
            .and_then(|scene| scene.nearest_cell(self.pointer.0, self.pointer.1));
        if cell.is_some_and(|cell| self.tabs.session_mut(active).select_drag(cell)) {
            self.invalidate();
        }
    }

    fn button(&mut self, state: ElementState, button: MouseButton) {
        let (x, y) = self.pointer;
        if state == ElementState::Released {
            if button == MouseButton::Left
                && let Some(active) = self.tabs.active()
            {
                self.tabs.session_mut(active).select_end();
            }
            return;
        }
        let hit = self.layout.hit(x, y, self.tabs.len());
        if let Hit::List { line, col } = hit {
            return self.list_click(button, line, col);
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
            Hit::Terminal => {
                let Some(active) = self.tabs.active() else {
                    return;
                };
                self.set_focus(Focus::Terminal);
                let cell = self
                    .tabs
                    .front_scene()
                    .and_then(|scene| scene.cell_at(x, y));
                self.tabs.session_mut(active).select_start(cell);
            }
            Hit::List { .. } | Hit::Sidebar | Hit::TabBar | Hit::Status | Hit::Outside => {}
        }
    }

    /// The wheel scrolls what the pointer is over: the front tab's scrollback, or the session
    /// list.
    fn wheel(&mut self, delta: &MouseScrollDelta) {
        let cell_height = self.atlas.metrics().height;
        let rows = input::wheel_rows(delta, cell_height, &mut self.wheel_carry);
        let (x, y) = self.pointer;
        match self.layout.hit(x, y, self.tabs.len()) {
            Hit::Terminal => {
                if let Some(active) = self.tabs.active() {
                    // The frame at the new position comes back through the waker.
                    self.tabs.session_mut(active).scroll(rows);
                }
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
        self.dirty |= self.tabs.state_changes();
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
