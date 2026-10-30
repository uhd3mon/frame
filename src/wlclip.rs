//! The Wayland clipboard through wlr data-control, so no wl-copy is
//! needed. A Wayland clipboard is served by the program that set it, so
//! `frame _clip FILE MIME` runs detached: it takes the selection, prints
//! "ok" on stdout, then answers paste requests until another program sets
//! the clipboard. KWin, wlroots compositors and Hyprland have
//! data-control. Mutter (GNOME) doesn't: there we map a tiny transparent
//! window, take the keyboard-focus serial and set the selection with the
//! core wl_data_device (what wl-copy does), then unmap it and keep
//! serving. X11 (and GNOME if that fails, through Xwayland) owns the
//! CLIPBOARD selection in x11.rs.

use std::io::Write;
use std::os::fd::{AsRawFd, OwnedFd};
use std::sync::Arc;
use std::time::{Duration, Instant};
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{event_created_child, Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols_wlr::data_control::v1::client::{
    zwlr_data_control_device_v1 as device, zwlr_data_control_manager_v1 as manager, zwlr_data_control_offer_v1 as offer,
    zwlr_data_control_source_v1 as source,
};

struct Serve {
    data: Arc<Vec<u8>>,
    done: bool,
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Serve {
    fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<wl_seat::WlSeat, ()> for Serve {
    fn event(_: &mut Self, _: &wl_seat::WlSeat, _: wl_seat::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<manager::ZwlrDataControlManagerV1, ()> for Serve {
    fn event(_: &mut Self, _: &manager::ZwlrDataControlManagerV1, _: manager::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<device::ZwlrDataControlDeviceV1, ()> for Serve {
    fn event(st: &mut Self, _: &device::ZwlrDataControlDeviceV1, ev: device::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match ev {
            // Another program's selection: not ours to read.
            device::Event::DataOffer { id } => id.destroy(),
            device::Event::Finished => st.done = true,
            _ => {}
        }
    }

    event_created_child!(Serve, device::ZwlrDataControlDeviceV1, [
        device::EVT_DATA_OFFER_OPCODE => (offer::ZwlrDataControlOfferV1, ()),
    ]);
}

impl Dispatch<offer::ZwlrDataControlOfferV1, ()> for Serve {
    fn event(_: &mut Self, _: &offer::ZwlrDataControlOfferV1, _: offer::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<source::ZwlrDataControlSourceV1, ()> for Serve {
    fn event(st: &mut Self, _: &source::ZwlrDataControlSourceV1, ev: source::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match ev {
            source::Event::Send { fd, .. } => {
                // Each paste on its own thread, so a slow or stuck reader
                // never holds up the event loop (other pastes, Cancelled).
                let data = st.data.clone();
                std::thread::spawn(move || send_all(fd, &data, PASTE_TIMEOUT));
            }
            source::Event::Cancelled => st.done = true,
            _ => {}
        }
    }
}

/// How long one paste may take before we give up on its reader.
const PASTE_TIMEOUT: Duration = Duration::from_secs(10);

/// Write DATA to FD without blocking: on a full pipe wait (poll) for it to
/// drain, until TIMEOUT. Ok(false) = timed out. A reader that goes away
/// early is its problem (SIGPIPE is ignored; that write fails with EPIPE).
fn send_all(fd: OwnedFd, data: &[u8], timeout: Duration) -> std::io::Result<bool> {
    let raw = fd.as_raw_fd();
    // SAFETY: fcntl on an fd we own; only adds O_NONBLOCK.
    unsafe {
        let fl = libc::fcntl(raw, libc::F_GETFL);
        if fl < 0 || libc::fcntl(raw, libc::F_SETFL, fl | libc::O_NONBLOCK) < 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    let end = Instant::now() + timeout;
    let mut f = std::fs::File::from(fd);
    let mut rest = data;
    while !rest.is_empty() {
        match f.write(rest) {
            Ok(0) => return Err(std::io::ErrorKind::WriteZero.into()),
            Ok(n) => rest = &rest[n..],
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                let left = end.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return Ok(false);
                }
                let mut p = libc::pollfd { fd: raw, events: libc::POLLOUT, revents: 0 };
                // SAFETY: one valid pollfd for the duration of the call.
                let r = unsafe { libc::poll(&mut p, 1, left.as_millis().clamp(1, i32::MAX as u128) as i32) };
                if r < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Err(e) => return Err(e),
        }
    }
    Ok(true)
}

/// `frame _clip FILE MIME [data-control|focus|x11]`.
pub fn main(args: &[String]) -> i32 {
    let (file, mime, how) = match args {
        [f, m] => (f, m, "data-control"),
        [f, m, h] => (f, m, h.as_str()),
        _ => {
            eprintln!("usage: frame _clip FILE MIME [data-control|focus|x11]");
            return 2;
        }
    };
    let r = match how {
        "data-control" => serve(file, mime),
        "focus" => match serve_focus(file, mime) {
            // No focus (or no Wayland): Xwayland's clipboard, if there is one.
            Err(e) if std::env::var_os("DISPLAY").is_some() => serve_x11(file, mime).map_err(|x| format!("{e}; X11: {x}")),
            r => r,
        },
        "x11" => serve_x11(file, mime),
        _ => Err(format!("unknown clipboard method {how}")),
    };
    match r {
        Ok(()) => 0,
        Err(e) => {
            tell(&format!("error: {e}"));
            1
        }
    }
}

fn say_ok() {
    tell("ok");
}

/// One line to whoever started us. The pipe may be closed by then (the
/// caller stops listening after "ok" or its timeout): ignore that, no
/// panic like println! would.
fn tell(line: &str) {
    let mut o = std::io::stdout().lock();
    let _ = writeln!(o, "{line}").and_then(|_| o.flush());
}

fn serve_x11(file: &str, mime: &str) -> Result<(), String> {
    let data = std::fs::read(file).map_err(|e| format!("{file}: {e}"))?;
    crate::x11::serve_clipboard(&data, mime, say_ok)
}

fn serve(file: &str, mime: &str) -> Result<(), String> {
    let data = std::fs::read(file).map_err(|e| format!("{file}: {e}"))?;
    let conn = Connection::connect_to_env().map_err(|e| format!("Wayland: {e}"))?;
    let (globals, mut queue) = registry_queue_init::<Serve>(&conn).map_err(|e| format!("Wayland: {e}"))?;
    let qh = queue.handle();
    let mgr: manager::ZwlrDataControlManagerV1 =
        globals.bind(&qh, 1..=2, ()).map_err(|_| "the compositor has no wlr data-control".to_string())?;
    let seat: wl_seat::WlSeat = globals.bind(&qh, 1..=7, ()).map_err(|_| "no Wayland seat".to_string())?;
    let mut st = Serve { data: Arc::new(data), done: false };
    let dev = mgr.get_data_device(&seat, &qh, ());
    let src = mgr.create_data_source(&qh, ());
    src.offer(mime.to_string());
    dev.set_selection(Some(&src));
    queue.roundtrip(&mut st).map_err(|e| format!("Wayland: {e}"))?;
    if st.done {
        return Err("the compositor refused the selection".into());
    }
    say_ok();
    while !st.done {
        if queue.blocking_dispatch(&mut st).is_err() {
            break;
        }
    }
    src.destroy();
    if dev.version() >= 2 {
        dev.destroy();
    }
    Ok(())
}

// ------------------------------------------- GNOME: wl_data_device + focus

mod focus {
    use super::*;
    use std::os::fd::AsFd;
    use wayland_client::protocol::{
        wl_buffer, wl_compositor, wl_data_device, wl_data_device_manager, wl_data_offer, wl_data_source, wl_keyboard, wl_shm, wl_shm_pool,
        wl_surface,
    };
    use wayland_client::{delegate_noop, WEnum};
    use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};

    #[derive(Default)]
    pub struct F {
        pub data: Arc<Vec<u8>>,
        pub surface: Option<wl_surface::WlSurface>,
        pub configured: bool,
        /// The keyboard-enter serial on our window.
        pub serial: Option<u32>,
        pub done: bool,
        pub closed: bool,
    }

    impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for F {
        fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
    }

    impl Dispatch<wl_seat::WlSeat, ()> for F {
        fn event(_: &mut Self, seat: &wl_seat::WlSeat, ev: wl_seat::Event, _: &(), _: &Connection, qh: &QueueHandle<Self>) {
            if let wl_seat::Event::Capabilities { capabilities: WEnum::Value(c) } = ev {
                if c.contains(wl_seat::Capability::Keyboard) {
                    seat.get_keyboard(qh, ());
                }
            }
        }
    }

    impl Dispatch<wl_keyboard::WlKeyboard, ()> for F {
        fn event(st: &mut Self, _: &wl_keyboard::WlKeyboard, ev: wl_keyboard::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
            if let wl_keyboard::Event::Enter { serial, surface, .. } = ev {
                if st.surface.as_ref() == Some(&surface) {
                    st.serial = Some(serial);
                }
            }
        }
    }

    impl Dispatch<xdg_wm_base::XdgWmBase, ()> for F {
        fn event(_: &mut Self, wm: &xdg_wm_base::XdgWmBase, ev: xdg_wm_base::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
            if let xdg_wm_base::Event::Ping { serial } = ev {
                wm.pong(serial);
            }
        }
    }

    impl Dispatch<xdg_surface::XdgSurface, ()> for F {
        fn event(st: &mut Self, xs: &xdg_surface::XdgSurface, ev: xdg_surface::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
            if let xdg_surface::Event::Configure { serial } = ev {
                xs.ack_configure(serial);
                st.configured = true;
            }
        }
    }

    impl Dispatch<xdg_toplevel::XdgToplevel, ()> for F {
        fn event(st: &mut Self, _: &xdg_toplevel::XdgToplevel, ev: xdg_toplevel::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
            if let xdg_toplevel::Event::Close = ev {
                st.closed = true;
            }
        }
    }

    impl Dispatch<wl_data_device::WlDataDevice, ()> for F {
        fn event(_: &mut Self, _: &wl_data_device::WlDataDevice, ev: wl_data_device::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
            if let wl_data_device::Event::DataOffer { id } = ev {
                id.destroy(); // another program's selection: not ours to read
            }
        }

        event_created_child!(F, wl_data_device::WlDataDevice, [
            wl_data_device::EVT_DATA_OFFER_OPCODE => (wl_data_offer::WlDataOffer, ()),
        ]);
    }

    impl Dispatch<wl_data_source::WlDataSource, ()> for F {
        fn event(st: &mut Self, _: &wl_data_source::WlDataSource, ev: wl_data_source::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
            match ev {
                wl_data_source::Event::Send { fd, .. } => {
                    let data = st.data.clone();
                    std::thread::spawn(move || send_all(fd, &data, PASTE_TIMEOUT));
                }
                wl_data_source::Event::Cancelled => st.done = true,
                _ => {}
            }
        }
    }

    delegate_noop!(F: ignore wl_compositor::WlCompositor);
    delegate_noop!(F: ignore wl_surface::WlSurface);
    delegate_noop!(F: ignore wl_shm::WlShm);
    delegate_noop!(F: ignore wl_shm_pool::WlShmPool);
    delegate_noop!(F: ignore wl_buffer::WlBuffer);
    delegate_noop!(F: ignore wl_data_device_manager::WlDataDeviceManager);
    delegate_noop!(F: ignore wl_data_offer::WlDataOffer);

    pub fn serve(file: &str, mime: &str) -> Result<(), String> {
        let data = std::fs::read(file).map_err(|e| format!("{file}: {e}"))?;
        let conn = Connection::connect_to_env().map_err(|e| format!("Wayland: {e}"))?;
        let (globals, mut queue) = registry_queue_init::<F>(&conn).map_err(|e| format!("Wayland: {e}"))?;
        let qh = queue.handle();
        let e = |what: &str| format!("the compositor has no {what}");
        let comp: wl_compositor::WlCompositor = globals.bind(&qh, 1..=4, ()).map_err(|_| e("wl_compositor"))?;
        let shm: wl_shm::WlShm = globals.bind(&qh, 1..=1, ()).map_err(|_| e("wl_shm"))?;
        let wm: xdg_wm_base::XdgWmBase = globals.bind(&qh, 1..=2, ()).map_err(|_| e("xdg-shell"))?;
        let ddm: wl_data_device_manager::WlDataDeviceManager = globals.bind(&qh, 1..=3, ()).map_err(|_| e("data device manager"))?;
        let seat: wl_seat::WlSeat = globals.bind(&qh, 1..=7, ()).map_err(|_| e("seat"))?;
        let mut st = F { data: Arc::new(data), ..Default::default() };

        // A 1x1 fully transparent window: the compositor gives it focus,
        // and with focus comes the serial set_selection needs.
        let surface = comp.create_surface(&qh, ());
        let xs = wm.get_xdg_surface(&surface, &qh, ());
        let tl = xs.get_toplevel(&qh, ());
        tl.set_title("Frame".into());
        tl.set_app_id("frame".into());
        surface.commit();
        st.surface = Some(surface.clone());
        let file = crate::wl::memfd(4)?;
        let pool = shm.create_pool(file.as_fd(), 4, &qh, ());
        let buf = pool.create_buffer(0, 1, 1, 4, wl_shm::Format::Argb8888, &qh, ());
        pool.destroy();
        let dev = ddm.get_data_device(&seat, &qh, ());

        let end = Instant::now() + Duration::from_secs(2);
        let mut attached = false;
        while st.serial.is_none() && !st.closed {
            if st.configured && !attached {
                surface.attach(Some(&buf), 0, 0);
                surface.commit();
                attached = true;
            }
            if Instant::now() > end {
                break;
            }
            queue.roundtrip(&mut st).map_err(|e| format!("Wayland: {e}"))?;
            if st.serial.is_none() {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        let serial = st.serial;
        let src = ddm.create_data_source(&qh, ());
        src.offer(mime.to_string());
        if let Some(s) = serial {
            dev.set_selection(Some(&src), s);
        }
        // Done with the window either way.
        tl.destroy();
        xs.destroy();
        surface.destroy();
        buf.destroy();
        st.surface = None;
        queue.roundtrip(&mut st).map_err(|e| format!("Wayland: {e}"))?;
        if serial.is_none() {
            return Err("the window never got keyboard focus, so the clipboard could not be set".into());
        }
        if st.done {
            return Err("the compositor refused the selection".into());
        }
        super::say_ok();
        while !st.done {
            if queue.blocking_dispatch(&mut st).is_err() {
                break;
            }
        }
        src.destroy();
        if dev.version() >= 2 {
            dev.release();
        }
        Ok(())
    }
}

fn serve_focus(file: &str, mime: &str) -> Result<(), String> {
    focus::serve(file, mime)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reader that never reads: the write gives up after the timeout
    /// instead of blocking; a reader that does read gets every byte.
    #[test]
    fn paste_never_blocks_on_a_stuck_reader() {
        let data = vec![7u8; 1 << 20]; // far more than a pipe holds
        let (rd, wr) = std::io::pipe().unwrap();
        let t = Instant::now();
        assert!(!send_all(OwnedFd::from(wr), &data, Duration::from_millis(300)).unwrap());
        assert!(t.elapsed() < Duration::from_secs(3), "{:?}", t.elapsed());
        drop(rd);

        let (mut rd, wr) = std::io::pipe().unwrap();
        let reader = std::thread::spawn(move || {
            let mut v = Vec::new();
            std::io::Read::read_to_end(&mut rd, &mut v).map(|_| v)
        });
        assert!(send_all(OwnedFd::from(wr), &data, Duration::from_secs(10)).unwrap());
        assert_eq!(reader.join().unwrap().unwrap(), data);
    }
}
