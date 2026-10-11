//! The winit event loop and the wgpu surface: one window showing one terminal session.
//!
//! This file only moves events and frames between winit, wgpu and the modules that hold the
//! logic (`input`, `session`, `select`, `marks`, `gfx`); those are tested without a window.
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

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow, bail};
use s2_core::bench::Sampler;
use s2_core::latency::{Event, Log};
use s2_core::{Pty, now_ns};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::window::{Window, WindowId};

use crate::cli::{Args, Present};
use crate::gfx::atlas::{Atlas, FontSpec};
use crate::gfx::grid::{Scene, Theme};
use crate::gfx::render::{Gpu, Renderer};
use crate::input::{self, Action};
use crate::marks::{Marks, PresentPath, WorkDone};
use crate::session::Session;

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
            if let Err(e) = running.redraw(Redraw::Woken) {
                self.error = Some(e);
                event_loop.exit();
            }
            if running.session.output_drained() {
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
            WindowEvent::Focused(focused) => running.session.focus(focused),
            WindowEvent::ModifiersChanged(mods) => running.mods = mods.state(),
            WindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } => running.key(t_event_ns, &event),
            WindowEvent::Ime(ime) => running.ime(t_event_ns, ime),
            WindowEvent::CursorMoved { position, .. } => running.pointer_moved(position),
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => running.left_button(state),
            WindowEvent::MouseWheel { delta, .. } => running.wheel(&delta),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(running) = &mut self.running else {
            return;
        };
        let now = Instant::now();
        if running.exit_by.is_some_and(|deadline| now >= deadline) {
            return event_loop.exit();
        }
        let poll = running.settle_marks().then_some(now + WORK_DONE_POLL);
        event_loop.set_control_flow(match (poll, running.exit_by) {
            (Some(poll), Some(exit)) => ControlFlow::WaitUntil(poll.min(exit)),
            (Some(at), None) | (None, Some(at)) => ControlFlow::WaitUntil(at),
            (None, None) => ControlFlow::Wait,
        });
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
}

struct Running {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    gpu: Gpu,
    renderer: Renderer,
    atlas: Atlas,
    scene: Scene,
    theme: Theme,
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
    /// Where the IME was last told the cursor is.
    ime_cell: Option<(u16, u16)>,
    /// The child has exited: leave at this time at the latest.
    exit_by: Option<Instant>,
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
        let mut atlas = Atlas::new(FontSpec::default(), scale as f32);
        atlas.set_max_height(gpu.device.limits().max_texture_dimension_2d);
        let (cols, rows) = atlas.metrics().grid_for(config.width, config.height);
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
            "s2-wgpu: adapter {:?} / {} / {} {}; format {:?}; present {}; scale {scale}; \
             refresh_rate_millihertz {:?}; grid {cols}x{rows}",
            info.backend,
            info.name,
            info.driver,
            info.driver_info,
            config.format,
            present.as_str(),
            refresh_rate_millihertz(&window),
        );
        Ok(Running {
            marks: log.clone().map(Marks::new),
            sampler: log.clone().map(Sampler::start),
            meta: Meta {
                present_mode: present.as_str(),
                load: args.load_label.as_str(),
                written: false,
            },
            paced: present == Present::Fifo,
            window,
            surface,
            config,
            gpu,
            renderer,
            atlas,
            scene: Scene::new(),
            theme: Theme::default(),
            session,
            log,
            clipboard: None,
            mods: ModifiersState::empty(),
            pointer: (-1.0, -1.0),
            wheel_carry: 0.0,
            wake_pending,
            woken: false,
            dirty: true,
            ime_cell: None,
            exit_by: None,
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
        self.regrid();
    }

    /// A new scale factor: every glyph is rasterised again at the new size.
    fn rescaled(&mut self, scale: f64) {
        if self.atlas.set_scale(scale as f32) {
            eprintln!("s2-wgpu: scale factor {scale}");
            self.regrid();
        }
    }

    /// Tells the emulator how many cells fit now (`InputMsg::Resize` if that changed).
    fn regrid(&mut self) {
        let (cols, rows) = self
            .atlas
            .metrics()
            .grid_for(self.config.width, self.config.height);
        self.session.resize(cols, rows);
        self.invalidate();
    }

    fn key(&mut self, t_key_ns: u128, event: &KeyEvent) {
        let pressed = event.state == ElementState::Pressed;
        let action = input::key_action(
            &event.logical_key,
            event.text.as_deref(),
            self.mods,
            pressed,
            event.repeat,
        );
        match action {
            Action::Send(key) => {
                self.session.key(t_key_ns, key);
            }
            Action::Copy => self.copy(),
            Action::Paste => self.paste(),
            Action::Ignore => {}
        }
    }

    fn ime(&mut self, t_key_ns: u128, ime: Ime) {
        let changed = match ime {
            Ime::Preedit(text, _) => self.session.set_preedit(&text),
            Ime::Commit(text) => {
                self.session.ime_commit(t_key_ns, &text);
                true
            }
            Ime::Disabled => self.session.set_preedit(""),
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
                Err(e) => eprintln!("s2-wgpu: no clipboard: {e}"),
            }
        }
        self.clipboard.as_mut()
    }

    fn copy(&mut self) {
        let Some(text) = self.session.selected_text() else {
            return;
        };
        if let Some(clipboard) = self.clipboard()
            && let Err(e) = clipboard.set_text(text)
        {
            eprintln!("s2-wgpu: copy failed: {e}");
        }
    }

    fn paste(&mut self) {
        let Some(clipboard) = self.clipboard() else {
            return;
        };
        match clipboard.get_text() {
            Ok(text) => self.session.paste(&text),
            Err(e) => eprintln!("s2-wgpu: paste failed: {e}"),
        }
    }

    fn pointer_moved(&mut self, position: PhysicalPosition<f64>) {
        self.pointer = (position.x as f32, position.y as f32);
        if !self.session.selecting() {
            return;
        }
        let cell = self.scene.nearest_cell(self.pointer.0, self.pointer.1);
        if cell.is_some_and(|cell| self.session.select_drag(cell)) {
            self.invalidate();
        }
    }

    fn left_button(&mut self, state: ElementState) {
        match state {
            ElementState::Pressed => {
                let cell = self.scene.cell_at(self.pointer.0, self.pointer.1);
                if self.session.select_start(cell) {
                    self.invalidate();
                }
            }
            ElementState::Released => self.session.select_end(),
        }
    }

    fn wheel(&mut self, delta: &MouseScrollDelta) {
        let cell_height = self.atlas.metrics().height;
        let rows = input::wheel_rows(delta, cell_height, &mut self.wheel_carry);
        // The frame at the new position comes back through the waker.
        self.session.scroll(rows);
    }

    /// Takes the waiting frames and, if there is something new to show, draws and presents.
    fn redraw(&mut self, cause: Redraw) -> anyhow::Result<()> {
        let new_frame = self
            .session
            .take_frames(&mut self.scene, &mut self.atlas, &self.theme);
        if !new_frame && !self.dirty && cause == Redraw::Woken {
            return Ok(());
        }
        let Some(frame) = self.session.frame() else {
            return Ok(());
        };
        if !new_frame {
            // Same frame as before: this picks up a new scale factor (atlas generation).
            self.scene.update_cells(frame, &mut self.atlas, &self.theme);
        }
        let overlays = self.session.overlays();
        self.scene
            .update_overlays(frame, &mut self.atlas, &self.theme, &overlays);
        self.renderer.prepare(
            &self.gpu.device,
            &self.gpu.queue,
            &mut self.atlas,
            &self.scene.layers(),
            (self.config.width, self.config.height),
        );

        let (texture, suboptimal) = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => (texture, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => (texture, true),
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.gpu.device, &self.config);
                self.invalidate();
                return Ok(());
            }
            // Not visible, or the compositor is not taking frames: draw at the next redraw.
            // The frames taken above keep their keys in the log until a frame is presented.
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                self.dirty = true;
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Validation => bail!("the surface rejected the frame"),
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
        self.renderer.draw(&mut encoder, &view, self.theme.bg);
        drop(view);
        let seq = frame.seq;
        let cursor = frame.cursor.map(|cursor| (cursor.col, cursor.row));

        self.write_meta();
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
        self.dirty = false;
        self.settle_marks();
        if suboptimal {
            self.surface.configure(&self.gpu.device, &self.config);
        }
        self.place_ime(cursor);
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

    /// The `meta` line, written once, right before the first presented frame: by then the
    /// window is on its monitor and has its real scale factor.
    fn write_meta(&mut self) {
        if std::mem::replace(&mut self.meta.written, true) {
            return;
        }
        let Some(log) = &self.log else {
            return;
        };
        let millihertz = refresh_rate_millihertz(&self.window);
        log.event(&Event::Meta {
            app: "wgpu".to_string(),
            git: env!("S2_GIT").to_string(),
            profile: if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            }
            .to_string(),
            // 0.0: the window system did not report a refresh rate.
            hz: millihertz.map_or(0.0, |mhz| mhz as f64 / 1000.0),
            scale: self.window.scale_factor(),
            present_mode: self.meta.present_mode.to_string(),
            load: self.meta.load.to_string(),
        });
    }

    /// Keeps the input method's candidate window at the cursor cell.
    fn place_ime(&mut self, cursor: Option<(u16, u16)>) {
        if cursor == self.ime_cell {
            return;
        }
        self.ime_cell = cursor;
        if let Some((col, row)) = cursor {
            let m = self.atlas.metrics();
            self.window.set_ime_cursor_area(
                PhysicalPosition::new(col as f32 * m.width, row as f32 * m.height),
                PhysicalSize::new(m.width, m.height),
            );
        }
    }

    /// Writes what is still pending and flushes the log. Dropping `self` afterwards stops the
    /// emulator threads and closes the PTY.
    fn shutdown(mut self) {
        // Stops the sampler thread first, so its last line is in before the final flush.
        drop(self.sampler.take());
        self.write_meta();
        let _ = self.gpu.device.poll(wgpu::PollType::Poll);
        if let Some(marks) = &mut self.marks {
            marks.finish();
        }
        if let Some(log) = &self.log {
            log.flush();
        }
    }
}

fn refresh_rate_millihertz(window: &Window) -> Option<u32> {
    window.current_monitor()?.refresh_rate_millihertz()
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
