//! The winit event loop and the wgpu surface: one window showing one terminal session.
//!
//! This file only moves events and frames between winit, wgpu and the modules that hold the
//! logic (`input`, `session`, `select`, `marks`, `gfx`, `ui`); those are tested without a
//! window.
//!
//! Besides the terminal there is a small UI (`ui`: sidebar list, a one-field form, a context
//! menu). Every key and IME event goes through `ui::key_event` / `ui::ime_event`, which hand
//! it to the session only while the terminal has the keyboard focus; whatever else the UI
//! wants done comes back as a `ui::Effect` (`Running::apply`).
//!
//! Frame path: the emulator thread publishes a frame and calls the waker → `UserEvent::Wake`
//! → `request_redraw` → `RedrawRequested` takes the frame(s), builds quads, submits and
//! presents. When the child exits, the loop runs on until the frame with the child's last
//! output has been drawn (`Session::begin_exit`), then ends.
//!
//! Pacing: with `--present fifo` the app calls `Window::pre_present_notify` before each
//! present, so on Wayland winit delivers `RedrawRequested` at most once per compositor frame
//! and the main thread waits in the event loop — where key events get their timestamp — not
//! inside `present()`. With `mailbox` and `immediate` it does not, and redraws are as fast as
//! frames arrive.

use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow, bail};
use s2_core::bench::Sampler;
use s2_core::latency::Log;
use s2_core::{Pty, now_ns};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::window::{Window, WindowId};

use crate::cli::{Args, Present};
use crate::gfx::atlas::Atlas;
use crate::gfx::grid::Scene;
use crate::gfx::render::{Gpu, Renderer};
use crate::input;
use crate::marks::{Marks, PresentPath, WorkDone};
use crate::meta::{self, Refresh};
use crate::session::Session;
use crate::surface::{self, Acquired, Next};
use crate::ui::draw::{self, UiLayers};
use crate::ui::layout::Layout;
use crate::ui::theme::{ZoomDir, zoom_step};
use crate::ui::{self, Effect, Ui};

/// How often the device is polled while a frame's work-done callback is outstanding; the
/// resolution of `t_work_done_ns`.
const WORK_DONE_POLL: Duration = Duration::from_micros(500);

/// After the child exits: the longest the app waits for the frame with the child's last output
/// (`Session::output_drained`). A safety net: a release build has that frame within milliseconds.
const EXIT_WAIT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug)]
pub enum UserEvent {
    /// The emulator has a frame waiting, or the child exited. May be spurious.
    Wake,
}

/// Runs the app until the window is closed or the child exits.
pub fn run(args: Args) -> anyhow::Result<()> {
    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .build()
        .context("no display to open a window on (Wayland or X11)")?;
    let mut app = App {
        args,
        proxy: event_loop.create_proxy(),
        running: None,
        error: None,
    };
    event_loop.run_app(&mut app).context("event loop")?;
    app.error.map_or(Ok(()), Err)
}

struct App {
    args: Args,
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
        match Running::start(event_loop, &self.args, self.proxy.clone()) {
            Ok(running) => self.running = Some(running),
            Err(e) => {
                self.error = Some(e);
                event_loop.exit();
            }
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, UserEvent::Wake: UserEvent) {
        let Some(running) = &mut self.running else {
            return;
        };
        // Cleared before the frame is taken: a publish after this sends a new event.
        running.wake_pending.store(false, Ordering::SeqCst);
        if running.session.exited() {
            if running.session.begin_exit() {
                running.exit_by = Some(Instant::now() + EXIT_WAIT);
            }
            // Drawn here, not at the next compositor frame: the loop is about to end.
            // `about_to_wait` decides when (a redraw can end without a present).
            if let Err(e) = running.redraw(Redraw::Woken) {
                self.error = Some(e);
                event_loop.exit();
            }
        } else {
            running.woken = true;
            running.window.request_redraw();
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
                // Not `eprintln!`, which panics when stderr is a terminal that was closed.
                let _ = writeln!(std::io::stderr(), "s2-wgpu: focus {focused}");
                running.window_focused = focused;
                running.sync_focus();
            }
            WindowEvent::ModifiersChanged(mods) => running.mods = mods.state(),
            WindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } => running.key(t_event_ns, &event),
            WindowEvent::Ime(ime) => running.ime(t_event_ns, ime),
            WindowEvent::CursorMoved { position, .. } => {
                running.pointer_moved(t_event_ns, position)
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => running.left_button(t_event_ns, state),
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Right,
                ..
            } => running.right_button(t_event_ns),
            WindowEvent::MouseWheel { delta, .. } => running.wheel(t_event_ns, &delta),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(running) = &mut self.running else {
            return;
        };
        let now = Instant::now();
        if let Some(deadline) = running.exit_by {
            let drained = running.session.output_drained();
            if surface::may_exit(drained, running.shown(), now >= deadline) {
                return event_loop.exit();
            }
        }
        if running.retry_at.is_some_and(|at| now >= at) {
            running.retry_at = None;
            running.invalidate();
        }
        if running.meta.first_try.is_some() {
            // The window may have entered its monitor since the first present.
            running.write_meta(false);
        }
        let poll = running.settle_marks().then_some(now + WORK_DONE_POLL);
        let next = [
            poll,
            running.exit_by,
            running.retry_at,
            running.meta_deadline(),
        ]
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
    /// Requested after an emulator wake-up: nothing to do if no frame was waiting after all.
    Woken,
    /// The window system asked, or the app must draw whatever it has.
    Forced,
}

/// What goes into the `meta` line besides the values read when it is written.
struct Meta {
    present_mode: &'static str,
    load: &'static str,
    written: bool,
    /// When `write_meta` first ran: the refresh rate is waited for from then (`meta::META_WAIT`).
    first_try: Option<Instant>,
}

struct Running {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    gpu: Gpu,
    renderer: Renderer,
    atlas: Atlas,
    scene: Scene,
    /// The sidebar, the form and the context menu; also who has the keyboard focus.
    ui: Ui,
    /// Where the UI and the terminal are, for the current size, scale and font size.
    layout: Layout,
    /// The UI's quads, rebuilt only when it changed.
    layers: UiLayers,
    session: Session,
    log: Option<Log>,
    marks: Option<Marks<Log>>,
    sampler: Option<Sampler>,
    meta: Meta,
    /// `pre_present_notify` before each present (fifo only; see the module comment).
    paced: bool,
    clipboard: Option<arboard::Clipboard>,
    mods: ModifiersState,
    pointer: (f32, f32),
    wheel_carry: f32,
    /// A `Wake` event is on its way; the waker sends no second one.
    wake_pending: Arc<AtomicBool>,
    /// The pending `RedrawRequested` was asked for by a wake-up.
    woken: bool,
    /// Something other than a new frame changed the picture (overlay, size, scale).
    dirty: bool,
    /// Where the IME was last told the text cursor is (window pixels).
    ime_at: Option<(f32, f32)>,
    /// The window has the keyboard focus.
    window_focused: bool,

    /// The child has exited: leave at this time at the latest.
    exit_by: Option<Instant>,
    /// `seq` of the frame presented last.
    shown_seq: Option<u64>,
    /// A redraw got no texture and is to be tried again at this time.
    retry_at: Option<Instant>,
}

impl Running {
    fn start(
        event_loop: &ActiveEventLoop,
        args: &Args,
        proxy: EventLoopProxy<UserEvent>,
    ) -> anyhow::Result<Running> {
        let attributes = Window::default_attributes()
            .with_title("s2-wgpu")
            .with_inner_size(LogicalSize::new(960.0, 600.0));
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .context("create window")?,
        );
        window.set_ime_allowed(true);

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
        let wanted = match args.present {
            Present::Fifo => wgpu::PresentMode::Fifo,
            Present::Mailbox => wgpu::PresentMode::Mailbox,
            Present::Immediate => wgpu::PresentMode::Immediate,
        };
        let present = if caps.present_modes.contains(&wanted) {
            args.present
        } else {
            eprintln!(
                "s2-wgpu: --present {} is not offered here ({:?}); using fifo",
                args.present.as_str(),
                caps.present_modes
            );
            Present::Fifo
        };
        config.present_mode = match present {
            Present::Fifo => wgpu::PresentMode::Fifo,
            Present::Mailbox => wgpu::PresentMode::Mailbox,
            Present::Immediate => wgpu::PresentMode::Immediate,
        };
        if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
            config.alpha_mode = wgpu::CompositeAlphaMode::Opaque;
        }
        // The shortest queue of frames waiting for the display the surface allows.
        config.desired_maximum_frame_latency = 1;
        surface.configure(&gpu.device, &config);

        let scale = window.scale_factor();
        // `S2_WGPU_NO_UI=1`: the terminal alone, to measure what the UI costs per frame.
        let ui = Ui::with_ui(meta::ui_enabled(
            std::env::var(meta::NO_UI_ENV).ok().as_deref(),
        ));
        let mut atlas = Atlas::new(ui.theme.font.clone(), scale as f32);
        atlas.set_max_height(gpu.device.limits().max_texture_dimension_2d);
        let layout = ui.layout(
            (config.width, config.height),
            atlas.scale(),
            &atlas.metrics(),
        );
        let (cols, rows) = layout.grid;
        let mut scene = Scene::new();
        scene.set_origin(layout.terminal.x, layout.terminal.y);
        let renderer = Renderer::new(&gpu.device, config.format);

        let log = match &args.bench {
            Some(dir) => {
                Some(Log::create(dir).with_context(|| format!("--bench {}", dir.display()))?)
            }
            None => None,
        };
        let pty =
            Pty::spawn(&args.cmd, cols, rows).with_context(|| format!("--cmd {:?}", args.cmd))?;
        let session = Session::start(pty, cols, rows, log.clone(), args.photon)?;
        let wake_pending = Arc::new(AtomicBool::new(false));
        let pending = wake_pending.clone();
        session.set_waker(Box::new(move || {
            if !pending.swap(true, Ordering::SeqCst) {
                let _ = proxy.send_event(UserEvent::Wake);
            }
        }));

        eprintln!(
            "s2-wgpu: adapter {:?} / {} / {} {}; format {:?}; present {}; grid {cols}x{rows}",
            info.backend,
            info.name,
            info.driver,
            info.driver_info,
            config.format,
            present.as_str(),
        );
        Ok(Running {
            marks: log.clone().map(Marks::new),
            sampler: log.clone().map(Sampler::start),
            meta: Meta {
                present_mode: present.as_str(),
                load: args.load_label.as_str(),
                written: false,
                first_try: None,
            },
            paced: present == Present::Fifo,
            window,
            surface,
            config,
            gpu,
            renderer,
            atlas,
            scene,
            ui,
            layout,
            layers: UiLayers::new(),
            session,
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
            exit_by: None,
            shown_seq: None,
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
            eprintln!("s2-wgpu: scale factor {scale}");
            self.relayout();
        }
    }

    /// Ctrl+= / Ctrl+−: one step of the font size; like a scale change for the atlas.
    fn zoom(&mut self, dir: ZoomDir) {
        let size = zoom_step(self.atlas.font_size(), dir);
        if self.atlas.set_font_size(size) {
            eprintln!("s2-wgpu: font size {size}");
            self.relayout();
        }
    }

    /// Window size, scale factor or font size changed: lays the window out again, moves the
    /// grid and tells the emulator how many cells fit now (`InputMsg::Resize` if that changed).
    fn relayout(&mut self) {
        self.layout = self.ui.layout(
            (self.config.width, self.config.height),
            self.atlas.scale(),
            &self.atlas.metrics(),
        );
        self.scene
            .set_origin(self.layout.terminal.x, self.layout.terminal.y);
        self.ui.list.fit(self.layout.visible_rows);
        let (cols, rows) = self.layout.grid;
        self.session.resize(cols, rows);
        self.invalidate();
    }

    fn key(&mut self, t_key_ns: u128, event: &KeyEvent) {
        let effect = ui::key_event(
            &mut self.ui,
            &mut self.session,
            &self.layout,
            t_key_ns,
            &event.logical_key,
            event.text.as_deref(),
            self.mods,
            event.state == ElementState::Pressed,
            event.repeat,
        );
        self.apply(t_key_ns, effect);
    }

    fn ime(&mut self, t_key_ns: u128, ime: Ime) {
        let effect = ui::ime_event(&mut self.ui, &mut self.session, t_key_ns, &ime);
        self.apply(t_key_ns, effect);
    }

    /// Does what the UI asked for. `t_event_ns`: when the event behind it was received.
    fn apply(&mut self, t_event_ns: u128, effect: Effect) {
        match effect {
            Effect::None => {}
            Effect::Redraw => self.invalidate(),
            Effect::SetClipboard(text) => {
                self.set_clipboard(text);
                self.invalidate();
            }
            Effect::PasteField => {
                if let Some(text) = self.clipboard_text()
                    && self.ui.paste(&text) == Effect::Redraw
                {
                    self.invalidate();
                }
            }
            Effect::TermCopy => self.copy(),
            Effect::TermPaste => self.paste(),
            Effect::TermClear => {
                self.session.key(t_event_ns, ui::menu::clear_key());
            }
            Effect::TermPress => {
                let cell = self.scene.cell_at(self.pointer.0, self.pointer.1);
                if self.session.select_start(cell) {
                    self.invalidate();
                }
            }
            Effect::Zoom(dir) => self.zoom(dir),
        }
        self.sync_focus();
    }

    /// After anything that may have moved the keyboard focus, opened or closed the menu, or
    /// changed the window's focus: does what `Ui::sync_focus` says (the decisions are there).
    fn sync_focus(&mut self) {
        let sync = self.ui.sync_focus(self.window_focused);
        if sync.end_terminal_preedit {
            self.session.set_preedit("");
        }
        if let Some(allowed) = sync.ime_allowed {
            self.window.set_ime_allowed(allowed);
        }
        if let Some(focused) = sync.tell_core {
            self.session.focus(focused);
        }
        if sync.redraw {
            self.invalidate();
        }
    }

    fn clipboard(&mut self) -> Option<&mut arboard::Clipboard> {
        if self.clipboard.is_none() {
            match arboard::Clipboard::new() {
                Ok(clipboard) => self.clipboard = Some(clipboard),
                Err(e) => eprintln!("s2-wgpu: no clipboard: {e}"),
            }
        }
        self.clipboard.as_mut()
    }

    fn set_clipboard(&mut self, text: String) {
        if let Some(clipboard) = self.clipboard()
            && let Err(e) = clipboard.set_text(text)
        {
            eprintln!("s2-wgpu: copy failed: {e}");
        }
    }

    fn clipboard_text(&mut self) -> Option<String> {
        match self.clipboard()?.get_text() {
            Ok(text) => Some(text),
            Err(e) => {
                eprintln!("s2-wgpu: paste failed: {e}");
                None
            }
        }
    }

    /// Copies the terminal's selection.
    fn copy(&mut self) {
        if let Some(text) = self.session.selected_text() {
            self.set_clipboard(text);
        }
    }

    /// Pastes the clipboard into the terminal.
    fn paste(&mut self) {
        if let Some(text) = self.clipboard_text() {
            self.session.paste(&text);
        }
    }

    fn pointer_moved(&mut self, t_event_ns: u128, position: PhysicalPosition<f64>) {
        self.pointer = (position.x as f32, position.y as f32);
        let effect = self
            .ui
            .pointer(self.pointer.0, self.pointer.1, &self.layout);
        self.apply(t_event_ns, effect);
        if !self.session.selecting() {
            return;
        }
        let cell = self.scene.nearest_cell(self.pointer.0, self.pointer.1);
        if cell.is_some_and(|cell| self.session.select_drag(cell)) {
            self.invalidate();
        }
    }

    fn left_button(&mut self, t_event_ns: u128, state: ElementState) {
        match state {
            ElementState::Pressed => {
                // The UI decides whose press it is; the terminal's comes back as `TermPress`.
                let effect = self.ui.press(self.pointer.0, self.pointer.1, &self.layout);
                self.apply(t_event_ns, effect);
            }
            ElementState::Released => {
                self.session.select_end();
                self.ui.release();
            }
        }
    }

    /// Right button down: the context menu, on the terminal.
    fn right_button(&mut self, t_event_ns: u128) {
        let effect = self
            .ui
            .context(self.pointer.0, self.pointer.1, &self.layout);
        self.apply(t_event_ns, effect);
    }

    fn wheel(&mut self, t_event_ns: u128, delta: &MouseScrollDelta) {
        let cell_height = self.atlas.metrics().height;
        let rows = input::wheel_rows(delta, cell_height, &mut self.wheel_carry);
        let (x, y) = self.pointer;
        match self.ui.wheel(x, y, rows, &self.layout) {
            Some(effect) => self.apply(t_event_ns, effect),
            // The frame at the new position comes back through the waker.
            None => self.session.scroll(rows),
        }
    }

    /// Takes the waiting frames and, if there is something new to show, draws and presents.
    fn redraw(&mut self, cause: Redraw) -> anyhow::Result<()> {
        if let Some(marks) = &mut self.marks {
            marks.begin_redraw();
        }
        let theme = self.ui.theme.terminal;
        let new_frame = self
            .session
            .take_frames(&mut self.scene, &mut self.atlas, &theme);
        if !new_frame && !self.dirty && cause == Redraw::Woken {
            return Ok(());
        }
        let Some(frame) = self.session.frame() else {
            return Ok(());
        };
        if !new_frame {
            // Same frame as before: this picks up a new scale factor, font size or grid
            // origin (atlas generation, layout).
            self.scene.update_cells(frame, &mut self.atlas, &theme);
        }
        if self.dirty {
            // Something other than terminal output changed; that may have been the UI. A
            // redraw for a new frame alone (a key echo) leaves the UI's quads as they are.
            self.layers.invalidate();
        }
        let overlays = self.session.overlays();
        draw::compose(
            &mut self.scene,
            frame,
            &mut self.atlas,
            &overlays,
            &mut self.ui,
            &self.layout,
            &mut self.layers,
        );
        self.renderer.prepare(
            &self.gpu.device,
            &self.gpu.queue,
            &mut self.atlas,
            &draw::layer_order(&self.scene, &self.layers),
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
                label: Some("s2-wgpu frame"),
            });
        self.renderer.draw(&mut encoder, &view, theme.bg);
        drop(view);
        let seq = frame.seq;
        let cursor = frame.cursor.map(|cursor| (cursor.col, cursor.row));

        self.write_meta(false);
        let mut path = SurfacePath {
            queue: &self.gpu.queue,
            window: self.paced.then_some(&*self.window),
            commands: Some(encoder.finish()),
            texture: Some(texture),
        };
        match &mut self.marks {
            Some(marks) => marks.present_frame(seq, &mut path),
            None => {
                path.submit(WorkDone::default());
                path.present();
            }
        }
        self.session.presented();
        self.dirty = false;
        self.shown_seq = Some(seq);
        self.settle_marks();
        if suboptimal {
            self.surface.configure(&self.gpu.device, &self.config);
        }
        self.place_ime(cursor);
        Ok(())
    }

    /// Everything taken from the emulator has been presented.
    fn shown(&self) -> bool {
        !self.dirty && self.session.frame().map(|frame| frame.seq) == self.shown_seq
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

    /// The `meta` line, written once. Tried before every present and after every batch of
    /// events, from the first present on: by then the window has its real scale factor, but
    /// on Wayland it is on no monitor until after that present, so the line waits (at most
    /// `meta::META_WAIT`, or until exit) for a refresh rate — see `meta`.
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
            git: env!("S2_GIT"),
            profile: if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
            scale: self.window.scale_factor(),
            present_mode: self.meta.present_mode,
            load: self.meta.load,
        };
        meta::write(log, &fixed, &refresh, self.ui.enabled());
        // The log line is the record; this is for someone watching the terminal.
        eprintln!(
            "s2-wgpu: meta scale {} hz {} hz_source {} ui {} (after {} ms)",
            fixed.scale,
            refresh.hz(),
            refresh.source.as_str(),
            self.ui.enabled(),
            waited.as_millis(),
        );
    }

    /// When `write_meta` has to be tried again at the latest; `None` once it is written or
    /// before the first present.
    fn meta_deadline(&self) -> Option<Instant> {
        match (self.meta.written, self.meta.first_try) {
            (false, Some(first)) => Some(first + meta::META_WAIT),
            _ => None,
        }
    }

    /// Keeps the input method's candidate window at the text cursor: the terminal's cursor
    /// cell or the field's caret, whichever has the focus.
    fn place_ime(&mut self, cursor: Option<(u16, u16)>) {
        let at = self.ui.ime_anchor(&self.layout, cursor);
        if at == self.ime_at {
            return;
        }
        self.ime_at = at;
        if let Some((x, y)) = at {
            let (width, height) = self.layout.cell;
            self.window.set_ime_cursor_area(
                PhysicalPosition::new(x, y),
                PhysicalSize::new(width, height),
            );
        }
    }

    /// Writes what is still pending and flushes the log. Dropping `self` afterwards stops the
    /// emulator threads and closes the PTY.
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

/// The real `PresentPath`: `queue.submit` with the work-done callback, then `present`.
struct SurfacePath<'a> {
    queue: &'a wgpu::Queue,
    /// Set when redraws are paced by the compositor (see the module comment).
    window: Option<&'a Window>,
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
        if let Some(window) = self.window {
            window.pre_present_notify();
        }
        if let Some(texture) = self.texture.take() {
            self.queue.present(texture);
        }
    }
}
