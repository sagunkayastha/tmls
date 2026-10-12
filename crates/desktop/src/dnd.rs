//! Files dropped on the window, on Wayland. winit 0.30 has no drag and drop there (its
//! `DroppedFile` is never sent), so this listens itself: a `wl_data_device` of our own on
//! winit's connection (`Backend::from_foreign_display`), with an event queue of its own.
//!
//! A thread of its own handles that queue (`listen`): winit's loop sleeps while a drag is over
//! the window. A second, short one per drop reads the dropped `text/uri-list` from the pipe
//! the source writes to. Both hand what they have to the `sink`.
//!
//! What is accepted: a drag that offers `text/uri-list`, as a copy. Anything else is refused
//! (the pointer shows it) and a drop of it does nothing. Where on the window it lands does not
//! matter: the list is for the tab in front (`App::files_dropped`).
//!
//! A protocol error on a shared connection ends the whole app, so every request here is one
//! the protocol allows in that state: `set_actions` only on a drag's offer and only from
//! version 3, `finish` only after a drop whose type and action were accepted.

use std::ffi::c_void;
use std::io::Read;
use std::os::fd::{AsFd, AsRawFd};
use std::sync::Arc;

use wayland_backend::client::{Backend, WaylandError};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_data_device::{self, WlDataDevice};
use wayland_client::protocol::wl_data_device_manager::{DndAction, WlDataDeviceManager};
use wayland_client::protocol::wl_data_offer::{self, WlDataOffer};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum, event_created_child,
};

/// Set (to anything), drops are not listened for.
pub const NO_DND: &str = "TMLS_RS_NO_DND";

/// The type a file manager offers for files.
const URI_LIST: &str = "text/uri-list";

/// No more than this is read of one drop: a list of file names, not a file.
const MAX_LIST: u64 = 1024 * 1024;

/// Takes the `text/uri-list` of a drop, or why the listener stopped; called on a thread of
/// the listener's.
pub type Sink = Arc<dyn Fn(Result<String, String>) + Send + Sync>;

/// One offer the compositor introduced: a drag's, or the clipboard's (which is only kept to
/// be destroyed when the next one replaces it).
struct Offer {
    offer: WlDataOffer,
    types: Vec<String>,
    /// The action the compositor settled on is a real one (not "none").
    action: bool,
}

struct State {
    sink: Sink,
    /// Introduced by `data_offer`, not yet said to be a drag's or the clipboard's.
    fresh: Vec<Offer>,
    /// The drag over the window, and whether its type was accepted.
    drag: Option<(Offer, bool)>,
    selection: Option<Offer>,
}

impl State {
    fn take_fresh(&mut self, offer: &WlDataOffer) -> Option<Offer> {
        let at = self.fresh.iter().position(|fresh| &fresh.offer == offer)?;
        Some(self.fresh.swap_remove(at))
    }

    fn of(&mut self, offer: &WlDataOffer) -> Option<&mut Offer> {
        let drag = self.drag.as_mut().map(|(drag, _)| drag);
        self.fresh
            .iter_mut()
            .chain(drag)
            .chain(self.selection.as_mut())
            .find(|known| &known.offer == offer)
    }
}

/// The listener: a thread that reads the connection for its own queue until this is dropped.
pub struct Drops {
    /// Closing it is what stops the thread.
    stop: Option<std::io::PipeWriter>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drops {
    /// Starts listening on the Wayland connection behind `display` (a `wl_display*`, from the
    /// window's display handle).
    ///
    /// # Safety
    /// `display` must be a live `wl_display` and stay so until the `Drops` is dropped (which
    /// stops and joins its thread).
    pub unsafe fn start(display: *mut c_void, sink: Sink) -> Result<Drops, String> {
        // SAFETY: the caller's promise.
        let backend = unsafe { Backend::from_foreign_display(display.cast()) };
        let connection = Connection::from_backend(backend);
        let (globals, queue) = registry_queue_init::<State>(&connection)
            .map_err(|e| format!("the Wayland registry: {e}"))?;
        let handle = queue.handle();
        let seat: WlSeat = globals
            .bind(&handle, 1..=1, ())
            .map_err(|e| format!("no seat: {e}"))?;
        let manager: WlDataDeviceManager = globals
            .bind(&handle, 1..=3, ())
            .map_err(|e| format!("no data device manager: {e}"))?;
        let device = manager.get_data_device(&seat, &handle, ());
        let _ = connection.flush();
        let state = State {
            sink,
            fresh: Vec::new(),
            drag: None,
            selection: None,
        };
        let (stopped, stop) = std::io::pipe().map_err(|e| format!("a pipe: {e}"))?;
        let thread = std::thread::Builder::new()
            .name("tmls-rs-dnd".to_string())
            .spawn(move || {
                // Kept so the objects are not released while events for them can arrive.
                let _objects = (device, manager, seat);
                listen(&connection, queue, state, &stopped);
            })
            .map_err(|e| format!("a thread: {e}"))?;
        Ok(Drops {
            stop: Some(stop),
            thread: Some(thread),
        })
    }
}

impl Drop for Drops {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The thread: handles the events of its queue as they come, until `stopped` (the read end of
/// `Drops::stop`) hangs up or the connection fails. winit's loop cannot do this for us: while
/// a drag is over the window the pointer is not ours, nothing arrives for winit, and its loop
/// sleeps — the `enter` would be answered only after the drag had left.
///
/// Reading is shared with winit's thread the way libwayland means it to be: each side
/// announces its read (`prepare_read`), waits for the socket, and reads; whoever comes second
/// finds the bytes already sorted into the queues.
fn listen(
    connection: &Connection,
    mut queue: EventQueue<State>,
    mut state: State,
    stopped: &std::io::PipeReader,
) {
    let fail = |state: &State, why: String| (state.sink)(Err(format!("drops are off: {why}")));
    loop {
        if let Err(e) = queue.dispatch_pending(&mut state) {
            return fail(&state, e.to_string());
        }
        let _ = connection.flush();
        // `None`: events came in meanwhile — round again.
        let Some(guard) = queue.prepare_read() else {
            continue;
        };
        let mut fds = [
            libc::pollfd {
                fd: guard.connection_fd().as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: stopped.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // SAFETY: `fds` is two valid pollfd for the length of the call.
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
        if ready < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return;
        }
        // Dropped, or the socket is gone: the guard's drop takes the read back.
        if fds[1].revents != 0 || fds[0].revents & libc::POLLIN == 0 {
            return;
        }
        match guard.read() {
            Ok(_) => {}
            Err(WaylandError::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => return fail(&state, e.to_string()),
        }
    }
}

impl Dispatch<WlDataDevice, ()> for State {
    fn event(
        state: &mut State,
        _: &WlDataDevice,
        event: wl_data_device::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        match event {
            wl_data_device::Event::DataOffer { id } => state.fresh.push(Offer {
                offer: id,
                types: Vec::new(),
                action: false,
            }),
            wl_data_device::Event::Enter { serial, id, .. } => {
                if let Some((old, _)) = state.drag.take() {
                    old.offer.destroy();
                }
                let Some(drag) = id.and_then(|id| state.take_fresh(&id)) else {
                    return;
                };
                let files = drag.types.iter().any(|kind| kind == URI_LIST);
                drag.offer
                    .accept(serial, files.then(|| URI_LIST.to_string()));
                if files && drag.offer.version() >= 3 {
                    drag.offer.set_actions(DndAction::Copy, DndAction::Copy);
                }
                state.drag = Some((drag, files));
            }
            wl_data_device::Event::Leave => {
                if let Some((drag, _)) = state.drag.take() {
                    drag.offer.destroy();
                }
            }
            wl_data_device::Event::Drop => {
                let Some((drag, files)) = state.drag.take() else {
                    return;
                };
                // Before version 3 there is no action to wait for and no `finish`.
                let modern = drag.offer.version() >= 3;
                if files && (drag.action || !modern) {
                    receive(&drag.offer, state.sink.clone());
                    if modern {
                        drag.offer.finish();
                    }
                }
                drag.offer.destroy();
            }
            wl_data_device::Event::Selection { id } => {
                if let Some(old) = state.selection.take() {
                    old.offer.destroy();
                }
                state.selection = id.and_then(|id| state.take_fresh(&id));
            }
            _ => {}
        }
    }

    event_created_child!(State, WlDataDevice, [
        wl_data_device::EVT_DATA_OFFER_OPCODE => (WlDataOffer, ()),
    ]);
}

/// Asks the source for the list and reads it on a thread: the source writes when it gets to
/// it, and the window's thread must not wait for that.
fn receive(offer: &WlDataOffer, sink: Sink) {
    let Ok((reader, writer)) = std::io::pipe() else {
        return;
    };
    offer.receive(URI_LIST.to_string(), writer.as_fd());
    // Ours is closed now, so the read ends when the source closes its copy.
    drop(writer);
    let _ = std::thread::Builder::new()
        .name("tmls-rs-drop".to_string())
        .spawn(move || {
            let mut list = String::new();
            if reader.take(MAX_LIST).read_to_string(&mut list).is_ok() && !list.is_empty() {
                sink(Ok(list));
            }
        });
}

impl Dispatch<WlDataOffer, ()> for State {
    fn event(
        state: &mut State,
        offer: &WlDataOffer,
        event: wl_data_offer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        let Some(known) = state.of(offer) else {
            return;
        };
        match event {
            wl_data_offer::Event::Offer { mime_type } => known.types.push(mime_type),
            wl_data_offer::Event::Action { dnd_action } => {
                known.action = matches!(dnd_action, WEnum::Value(action) if !action.is_empty());
            }
            _ => {}
        }
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut State,
        _: &WlRegistry,
        _: <WlRegistry as Proxy>::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
    }
}

impl Dispatch<WlSeat, ()> for State {
    fn event(
        _: &mut State,
        _: &WlSeat,
        _: <WlSeat as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
    }
}

impl Dispatch<WlDataDeviceManager, ()> for State {
    fn event(
        _: &mut State,
        _: &WlDataDeviceManager,
        _: <WlDataDeviceManager as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
    }
}
