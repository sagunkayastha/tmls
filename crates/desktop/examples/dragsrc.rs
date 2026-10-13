//! A drag source for the drop check (`tooling/smoke/dnd.sh`): a plain window that, when a
//! pointer button goes down on it, starts a drag offering one type with one text.
//!
//!     dragsrc TYPE TEXT      e.g. dragsrc text/uri-list $'file:///tmp/a.png\r\n'
//!
//! Prints `ready` once the window is up, `sent` when the target asked for the data, then
//! `finished` (exit 0) when the target took the drop, or `cancelled` (exit 1) when it was
//! refused or dropped nowhere. Not part of the app.
//!
//! Only the Wayland build has the window: elsewhere this is an empty `main`.

#[cfg(not(target_os = "linux"))]
fn main() {}

#[cfg(target_os = "linux")]
use std::fs::File;
#[cfg(target_os = "linux")]
use std::io::Write;
#[cfg(target_os = "linux")]
use std::os::fd::{AsFd, FromRawFd, OwnedFd};

#[cfg(target_os = "linux")]
use wayland_client::globals::{GlobalListContents, registry_queue_init};
#[cfg(target_os = "linux")]
use wayland_client::protocol::wl_buffer::WlBuffer;
#[cfg(target_os = "linux")]
use wayland_client::protocol::wl_compositor::WlCompositor;
#[cfg(target_os = "linux")]
use wayland_client::protocol::wl_data_device::{self, WlDataDevice};
#[cfg(target_os = "linux")]
use wayland_client::protocol::wl_data_device_manager::{DndAction, WlDataDeviceManager};
#[cfg(target_os = "linux")]
use wayland_client::protocol::wl_data_offer::WlDataOffer;
#[cfg(target_os = "linux")]
use wayland_client::protocol::wl_data_source::{self, WlDataSource};
#[cfg(target_os = "linux")]
use wayland_client::protocol::wl_pointer::{self, WlPointer};
#[cfg(target_os = "linux")]
use wayland_client::protocol::wl_registry::WlRegistry;
#[cfg(target_os = "linux")]
use wayland_client::protocol::wl_seat::{self, WlSeat};
#[cfg(target_os = "linux")]
use wayland_client::protocol::wl_shm::{Format, WlShm};
#[cfg(target_os = "linux")]
use wayland_client::protocol::wl_shm_pool::WlShmPool;
#[cfg(target_os = "linux")]
use wayland_client::protocol::wl_surface::WlSurface;
#[cfg(target_os = "linux")]
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle, WEnum, delegate_noop, event_created_child,
};
#[cfg(target_os = "linux")]
use wayland_protocols::xdg::shell::client::xdg_surface::{self, XdgSurface};
#[cfg(target_os = "linux")]
use wayland_protocols::xdg::shell::client::xdg_toplevel::{self, XdgToplevel};
#[cfg(target_os = "linux")]
use wayland_protocols::xdg::shell::client::xdg_wm_base::{self, XdgWmBase};

#[cfg(target_os = "linux")]
struct State {
    kind: String,
    text: String,
    shm: WlShm,
    manager: WlDataDeviceManager,
    device: WlDataDevice,
    surface: WlSurface,
    pointer: Option<WlPointer>,
    /// The size the compositor asked for; 0 until it did.
    size: (i32, i32),
    drawn: bool,
    source: Option<WlDataSource>,
}

#[cfg(target_os = "linux")]
fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(kind), Some(text)) = (args.next(), args.next()) else {
        eprintln!("usage: dragsrc TYPE TEXT");
        std::process::exit(2);
    };
    let connection = Connection::connect_to_env().expect("a Wayland compositor");
    let (globals, mut queue) = registry_queue_init::<State>(&connection).expect("the registry");
    let handle = queue.handle();
    let compositor: WlCompositor = globals.bind(&handle, 1..=4, ()).expect("wl_compositor");
    let shm: WlShm = globals.bind(&handle, 1..=1, ()).expect("wl_shm");
    let base: XdgWmBase = globals.bind(&handle, 1..=1, ()).expect("xdg_wm_base");
    let seat: WlSeat = globals.bind(&handle, 1..=5, ()).expect("wl_seat");
    let manager: WlDataDeviceManager = globals
        .bind(&handle, 3..=3, ())
        .expect("data device manager");
    let device = manager.get_data_device(&seat, &handle, ());
    let surface = compositor.create_surface(&handle, ());
    let xdg = base.get_xdg_surface(&surface, &handle, ());
    let toplevel = xdg.get_toplevel(&handle, ());
    toplevel.set_title("dragsrc".to_string());
    toplevel.set_app_id("dragsrc".to_string());
    surface.commit();
    let mut state = State {
        kind,
        text,
        shm,
        manager,
        device,
        surface,
        pointer: None,
        size: (0, 0),
        drawn: false,
        source: None,
    };
    loop {
        queue
            .blocking_dispatch(&mut state)
            .expect("the Wayland connection");
    }
}

#[cfg(target_os = "linux")]
impl State {
    /// One flat colour, the size the compositor asked for.
    fn draw(&mut self, handle: &QueueHandle<State>) {
        let (width, height) = match self.size {
            (0, _) | (_, 0) => (300, 300),
            size => size,
        };
        let (stride, bytes) = (width * 4, (width * 4 * height) as usize);
        // SAFETY: memfd_create with a valid name returns a new descriptor or -1.
        let fd = unsafe { libc::memfd_create(c"dragsrc".as_ptr(), libc::MFD_CLOEXEC) };
        assert!(fd >= 0, "memfd_create");
        // SAFETY: `fd` is a descriptor this function just made and owns.
        let mut file = File::from(unsafe { OwnedFd::from_raw_fd(fd) });
        file.write_all(&[0x30, 0x60, 0x90, 0xff].repeat(bytes / 4))
            .expect("fill the buffer");
        let pool = self.shm.create_pool(file.as_fd(), bytes as i32, handle, ());
        let buffer = pool.create_buffer(0, width, height, stride, Format::Argb8888, handle, ());
        self.surface.attach(Some(&buffer), 0, 0);
        self.surface.damage(0, 0, width, height);
        self.surface.commit();
        if !self.drawn {
            self.drawn = true;
            println!("ready");
        }
    }
}

#[cfg(target_os = "linux")]
impl Dispatch<XdgSurface, ()> for State {
    fn event(
        state: &mut State,
        xdg: &XdgSurface,
        event: xdg_surface::Event,
        _: &(),
        _: &Connection,
        handle: &QueueHandle<State>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            xdg.ack_configure(serial);
            state.draw(handle);
        }
    }
}

#[cfg(target_os = "linux")]
impl Dispatch<XdgToplevel, ()> for State {
    fn event(
        state: &mut State,
        _: &XdgToplevel,
        event: xdg_toplevel::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        match event {
            xdg_toplevel::Event::Configure { width, height, .. } => state.size = (width, height),
            xdg_toplevel::Event::Close => std::process::exit(3),
            _ => {}
        }
    }
}

#[cfg(target_os = "linux")]
impl Dispatch<XdgWmBase, ()> for State {
    fn event(
        _: &mut State,
        base: &XdgWmBase,
        event: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            base.pong(serial);
        }
    }
}

#[cfg(target_os = "linux")]
impl Dispatch<WlSeat, ()> for State {
    fn event(
        state: &mut State,
        seat: &WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        handle: &QueueHandle<State>,
    ) {
        // The headless seat gains its pointer when the virtual pointer connects.
        if let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(caps),
        } = event
            && caps.contains(wl_seat::Capability::Pointer)
            && state.pointer.is_none()
        {
            state.pointer = Some(seat.get_pointer(handle, ()));
        }
    }
}

#[cfg(target_os = "linux")]
impl Dispatch<WlPointer, ()> for State {
    fn event(
        state: &mut State,
        _: &WlPointer,
        event: wl_pointer::Event,
        _: &(),
        _: &Connection,
        handle: &QueueHandle<State>,
    ) {
        let wl_pointer::Event::Button {
            serial,
            state: WEnum::Value(wl_pointer::ButtonState::Pressed),
            ..
        } = event
        else {
            return;
        };
        if state.source.is_some() {
            return;
        }
        let source = state.manager.create_data_source(handle, ());
        source.offer(state.kind.clone());
        source.set_actions(DndAction::Copy);
        state
            .device
            .start_drag(Some(&source), &state.surface, None, serial);
        state.source = Some(source);
        println!("dragging");
    }
}

#[cfg(target_os = "linux")]
impl Dispatch<WlDataSource, ()> for State {
    fn event(
        state: &mut State,
        _: &WlDataSource,
        event: wl_data_source::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        match event {
            wl_data_source::Event::Send { fd, .. } => {
                let _ = File::from(fd).write_all(state.text.as_bytes());
                println!("sent");
            }
            wl_data_source::Event::DndFinished => {
                println!("finished");
                std::process::exit(0);
            }
            wl_data_source::Event::Cancelled => {
                println!("cancelled");
                std::process::exit(1);
            }
            _ => {}
        }
    }
}

#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
delegate_noop!(State: ignore WlCompositor);
#[cfg(target_os = "linux")]
delegate_noop!(State: ignore WlShm);
#[cfg(target_os = "linux")]
delegate_noop!(State: ignore WlShmPool);
#[cfg(target_os = "linux")]
delegate_noop!(State: ignore WlBuffer);
#[cfg(target_os = "linux")]
delegate_noop!(State: ignore WlSurface);
#[cfg(target_os = "linux")]
delegate_noop!(State: ignore WlDataDeviceManager);
#[cfg(target_os = "linux")]
delegate_noop!(State: ignore WlDataOffer);

// The source's own window is a target too while the pointer is over it: its offers arrive
// here and are left alone.
#[cfg(target_os = "linux")]
impl Dispatch<WlDataDevice, ()> for State {
    fn event(
        _: &mut State,
        _: &WlDataDevice,
        _: wl_data_device::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
    }

    event_created_child!(State, WlDataDevice, [
        wl_data_device::EVT_DATA_OFFER_OPCODE => (WlDataOffer, ()),
    ]);
}
