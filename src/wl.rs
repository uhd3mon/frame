//! Wayland without helper programs: the outputs and their desktop
//! (logical) layout, shared-memory buffers, and screen capture through
//! ext-image-copy-capture-v1 or wlr-screencopy-unstable-v1 (grim's job).
//! The region selector (select.rs) shares this connection and state.

use crate::pixels::{self, Fmt, Rect, Shot};
use std::fs::File;
use std::os::fd::{AsFd, FromRawFd, OwnedFd};
use std::os::unix::fs::FileExt;
use std::time::{Duration, Instant};
use wayland_client::globals::{registry_queue_init, GlobalList, GlobalListContents};
use wayland_client::protocol::{wl_buffer, wl_output, wl_registry, wl_shm, wl_shm_pool};
use wayland_client::{delegate_noop, Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum};
use wayland_protocols::ext::image_capture_source::v1::client::{
    ext_image_capture_source_v1 as src, ext_output_image_capture_source_manager_v1 as src_mgr,
};
use wayland_protocols::ext::image_copy_capture::v1::client::{
    ext_image_copy_capture_frame_v1 as xframe, ext_image_copy_capture_manager_v1 as xmgr, ext_image_copy_capture_session_v1 as xsession,
};
use wayland_protocols::xdg::xdg_output::zv1::client::{zxdg_output_manager_v1 as xo_mgr, zxdg_output_v1 as xo};
use wayland_protocols_wlr::screencopy::v1::client::{zwlr_screencopy_frame_v1 as wframe, zwlr_screencopy_manager_v1 as wmgr};

pub struct Output {
    pub wl: wl_output::WlOutput,
    pub name: String,
    /// Where it sits on the desktop (xdg-output; else guessed from the mode).
    pub logical: Rect,
    pub scale: i32,
    /// wl_output.transform (0..=7).
    pub transform: u32,
    mode: (i32, i32),
    pos: (i32, i32),
    xdg_pos: Option<(i32, i32)>,
    xdg_size: Option<(i32, i32)>,
}

/// One capture in flight (we take one output at a time).
#[derive(Default)]
pub struct Cap {
    /// wlr: (format, width, height, stride) offers; ext: (format, w, h, 0).
    offers: Vec<(u32, u32, u32, u32)>,
    ext_size: Option<(u32, u32)>,
    /// All buffer constraints are in (wlr v3 buffer_done / v1-2 buffer, ext done).
    constraints_done: bool,
    y_invert: bool,
    transform: Option<u32>,
    ready: bool,
    failed: Option<String>,
}

pub struct State {
    pub outputs: Vec<Output>,
    pub cap: Cap,
    /// The region selector's part (select.rs).
    pub sel: crate::select::SelState,
}

pub struct Wl {
    /// Skip ext-image-copy-capture (FRAME_SCREENCOPY=wlr, for testing).
    pub wlr_only: bool,
    /// Selector without layer-shell, as on GNOME (FRAME_SELECT=xdg, for testing).
    pub xdg_only: bool,
    /// XDG_ACTIVATION_TOKEN from our launcher, for the selector's focus.
    pub activation_token: Option<String>,
    pub conn: Connection,
    pub queue: EventQueue<State>,
    pub globals: GlobalList,
    pub st: State,
    /// Bound on first use, shared by every buffer.
    shm: Option<wl_shm::WlShm>,
}

fn werr(e: impl std::fmt::Display) -> String {
    format!("Wayland: {e}")
}

/// The raw value of a protocol enum.
pub fn wenum<T: Into<u32>>(v: WEnum<T>) -> u32 {
    match v {
        WEnum::Value(v) => v.into(),
        WEnum::Unknown(u) => u,
    }
}

impl Wl {
    /// Connect to the compositor's socket at PATH and learn the outputs.
    pub fn connect_to(path: &std::path::Path) -> Result<Wl, String> {
        let sock = std::os::unix::net::UnixStream::connect(path).map_err(|e| format!("Wayland ({}): {e}", path.display()))?;
        let conn = Connection::from_socket(sock).map_err(werr)?;
        let (globals, queue) = registry_queue_init::<State>(&conn).map_err(werr)?;
        let mut wl = Wl { wlr_only: false, xdg_only: false, activation_token: None, conn, queue, globals, st: State { outputs: Vec::new(), cap: Cap::default(), sel: Default::default() }, shm: None };
        let qh = wl.queue.handle();
        let xdg: Option<xo_mgr::ZxdgOutputManagerV1> = wl.globals.bind(&qh, 1..=3, ()).ok();
        let list = wl.globals.contents().clone_list();
        for g in list.iter().filter(|g| g.interface == wl_output::WlOutput::interface().name) {
            let i = wl.st.outputs.len();
            let o: wl_output::WlOutput = wl.globals.registry().bind(g.name, g.version.min(4), &qh, i);
            if let Some(x) = &xdg {
                x.get_xdg_output(&o, &qh, i);
            }
            wl.st.outputs.push(Output {
                wl: o,
                name: String::new(),
                logical: Rect::new(0, 0, 0, 0),
                scale: 1,
                transform: 0,
                mode: (0, 0),
                pos: (0, 0),
                xdg_pos: None,
                xdg_size: None,
            });
        }
        wl.roundtrip()?;
        wl.roundtrip()?;
        for o in &mut wl.st.outputs {
            let (mut w, mut h) = o.mode;
            if o.transform & 1 == 1 {
                std::mem::swap(&mut w, &mut h);
            }
            let (lw, lh) = o.xdg_size.unwrap_or((w / o.scale.max(1), h / o.scale.max(1)));
            let (x, y) = o.xdg_pos.unwrap_or(o.pos);
            o.logical = Rect::new(x, y, lw, lh);
        }
        wl.st.outputs.retain(|o| !o.logical.is_empty());
        if wl.st.outputs.is_empty() {
            return Err("the compositor reports no monitors".into());
        }
        Ok(wl)
    }

    pub fn qh(&self) -> QueueHandle<State> {
        self.queue.handle()
    }

    /// The compositor's wl_shm (one binding for the connection).
    pub fn shm(&mut self) -> Result<wl_shm::WlShm, String> {
        if self.shm.is_none() {
            self.shm = Some(self.globals.bind(&self.queue.handle(), 1..=1, ()).map_err(|_| "no wl_shm")?);
        }
        Ok(self.shm.clone().expect("bound above"))
    }

    pub fn roundtrip(&mut self) -> Result<(), String> {
        self.queue.roundtrip(&mut self.st).map(|_| ()).map_err(werr)
    }

    /// Dispatch events until DONE says so, or fail after TIMEOUT.
    pub fn wait(&mut self, timeout: Duration, what: &str, done: impl Fn(&State) -> bool) -> Result<(), String> {
        let end = Instant::now() + timeout;
        while !done(&self.st) {
            self.conn.flush().map_err(werr)?;
            self.queue.dispatch_pending(&mut self.st).map_err(werr)?;
            if done(&self.st) {
                break;
            }
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(format!("the compositor did not answer ({what})"));
            }
            // Requests made while dispatching (get_pointer, ack_configure)
            // must go out before we sleep.
            self.conn.flush().map_err(werr)?;
            let Some(guard) = self.queue.prepare_read() else { continue };
            let mut p = libc::pollfd { fd: std::os::fd::AsRawFd::as_raw_fd(&guard.connection_fd()), events: libc::POLLIN, revents: 0 };
            // SAFETY: one valid pollfd for the duration of the call.
            let r = unsafe { libc::poll(&mut p, 1, left.as_millis().clamp(1, 1000) as i32) };
            if r > 0 {
                guard.read().map_err(werr)?;
            } // else: the guard drops (cancels the read) and we loop
        }
        Ok(())
    }

    pub fn has_global(&self, iface: &str) -> bool {
        self.globals.contents().with_list(|l| l.iter().any(|g| g.interface == iface))
    }

    /// Whether capture_output goes through ext-image-copy-capture.
    fn use_ext(&self) -> bool {
        !self.wlr_only
            && self.has_global(xmgr::ExtImageCopyCaptureManagerV1::interface().name)
            && self.has_global(src_mgr::ExtOutputImageCaptureSourceManagerV1::interface().name)
    }

    /// Which capture protocol capture_output uses.
    pub fn protocol(&self) -> &'static str {
        if self.use_ext() {
            "ext-image-copy-capture"
        } else if self.has_global(wmgr::ZwlrScreencopyManagerV1::interface().name) {
            "wlr-screencopy"
        } else {
            "none"
        }
    }

    /// Whether the compositor lets clients copy the screen.
    pub fn can_capture(&self) -> bool {
        (self.has_global(xmgr::ExtImageCopyCaptureManagerV1::interface().name)
            && self.has_global(src_mgr::ExtOutputImageCaptureSourceManagerV1::interface().name))
            || self.has_global(wmgr::ZwlrScreencopyManagerV1::interface().name)
    }

    /// The bounding box of all outputs.
    pub fn desktop(&self) -> Rect {
        Rect::bounds(self.st.outputs.iter().map(|o| o.logical)).unwrap_or(Rect::new(0, 0, 0, 0))
    }

    /// Capture the outputs that touch AREA (logical) and stitch them.
    pub fn capture_area(&mut self, area: Rect) -> Result<Shot, String> {
        Shot::join(self.capture_parts(area)?).ok_or_else(|| "nothing captured".into())
    }

    /// Each output that touches AREA, at its own density.
    pub fn capture_parts(&mut self, area: Rect) -> Result<Vec<Shot>, String> {
        let which: Vec<usize> = (0..self.st.outputs.len()).filter(|&i| self.st.outputs[i].logical.intersect(&area).is_some()).collect();
        if which.is_empty() {
            return Err("that area is not on any monitor".into());
        }
        let mut parts = Vec::new();
        for i in which {
            parts.push(self.capture_output(i)?);
        }
        Ok(parts)
    }

    /// One output, upright, at its own pixel density.
    pub fn capture_output(&mut self, i: usize) -> Result<Shot, String> {
        let qh = self.qh();
        let ext: Option<(xmgr::ExtImageCopyCaptureManagerV1, src_mgr::ExtOutputImageCaptureSourceManagerV1)> = if self.use_ext() {
            match (self.globals.bind(&qh, 1..=1, ()), self.globals.bind(&qh, 1..=1, ())) {
                (Ok(a), Ok(b)) => Some((a, b)),
                (a, b) => {
                    // Only one of the pair bound: give it back.
                    if let Ok(a) = a {
                        a.destroy();
                    }
                    if let Ok(b) = b {
                        b.destroy();
                    }
                    None
                }
            }
        } else {
            None
        };
        let img = match ext {
            Some((m, s)) => {
                let r = self.capture_ext(&m, &s, i);
                m.destroy();
                s.destroy();
                r?
            }
            None => {
                let m: wmgr::ZwlrScreencopyManagerV1 = self
                    .globals
                    .bind(&qh, 1..=3, ())
                    .map_err(|_| "this compositor offers no screen capture (ext-image-copy-capture or wlr-screencopy)".to_string())?;
                let r = self.capture_wlr(&m, i);
                m.destroy();
                r?
            }
        };
        Ok(Shot { img, area: self.st.outputs[i].logical })
    }

    fn capture_wlr(&mut self, m: &wmgr::ZwlrScreencopyManagerV1, i: usize) -> Result<image::RgbaImage, String> {
        let qh = self.qh();
        self.st.cap = Cap::default();
        let v3 = m.version() >= 3;
        let frame = m.capture_output(0, &self.st.outputs[i].wl, &qh, v3);
        let r = (|| {
            self.wait(Duration::from_secs(5), "screencopy buffer", |s| s.cap.constraints_done || s.cap.failed.is_some())?;
            if let Some(e) = self.st.cap.failed.take() {
                return Err(e);
            }
            let (code, w, h, stride) = pick(&self.st.cap.offers).ok_or("the compositor offers no pixel format we can read")?;
            let buf = ShmBuf::new(&self.shm()?, &qh, w, h, stride, code)?;
            frame.copy(&buf.wl);
            let r = self.wait(Duration::from_secs(5), "screencopy", |s| s.cap.ready || s.cap.failed.is_some());
            buf.wl.destroy();
            r?;
            if let Some(e) = self.st.cap.failed.take() {
                return Err(e);
            }
            let data = buf.read()?;
            let fmt = Fmt::from_wl(code).ok_or("unknown pixel format")?.opaque();
            let img = pixels::from_raw(w, h, stride as usize, fmt, &data, self.st.cap.y_invert)?;
            Ok(pixels::apply_transform(img, self.st.outputs[i].transform))
        })();
        frame.destroy();
        r.map_err(|e| format!("screen capture of {}: {e}", self.st.outputs[i].name))
    }

    fn capture_ext(&mut self, m: &xmgr::ExtImageCopyCaptureManagerV1, s: &src_mgr::ExtOutputImageCaptureSourceManagerV1, i: usize) -> Result<image::RgbaImage, String> {
        let qh = self.qh();
        self.st.cap = Cap::default();
        let source = s.create_source(&self.st.outputs[i].wl, &qh, ());
        let session = m.create_session(&source, xmgr::Options::empty(), &qh, ());
        let r = (|| {
            self.wait(Duration::from_secs(5), "capture session", |s| s.cap.constraints_done || s.cap.failed.is_some())?;
            if let Some(e) = self.st.cap.failed.take() {
                return Err(e);
            }
            let (w, h) = self.st.cap.ext_size.ok_or("the compositor sent no buffer size")?;
            let offers: Vec<_> = self.st.cap.offers.iter().map(|&(c, ..)| (c, w, h, w * 4)).collect();
            let (code, w, h, stride) = pick(&offers).ok_or("the compositor offers no pixel format we can read")?;
            let buf = ShmBuf::new(&self.shm()?, &qh, w, h, stride, code)?;
            let frame = session.create_frame(&qh, ());
            frame.attach_buffer(&buf.wl);
            frame.damage_buffer(0, 0, w as i32, h as i32);
            frame.capture();
            let r = self.wait(Duration::from_secs(5), "capture", |s| s.cap.ready || s.cap.failed.is_some());
            frame.destroy();
            buf.wl.destroy();
            r?;
            if let Some(e) = self.st.cap.failed.take() {
                return Err(e);
            }
            let data = buf.read()?;
            let fmt = Fmt::from_wl(code).ok_or("unknown pixel format")?.opaque();
            let img = pixels::from_raw(w, h, stride as usize, fmt, &data, false)?;
            // The frame says how its buffer is turned; it is normally the
            // output's own transform.
            Ok(pixels::apply_transform(img, self.st.cap.transform.unwrap_or(self.st.outputs[i].transform)))
        })();
        session.destroy();
        source.destroy();
        r.map_err(|e| format!("screen capture of {}: {e}", self.st.outputs[i].name))
    }
}

/// The best offer we can read (8-bit first, then no alpha).
fn pick(offers: &[(u32, u32, u32, u32)]) -> Option<(u32, u32, u32, u32)> {
    offers.iter().filter_map(|&o| Fmt::from_wl(o.0).map(|f| (f.rank(), o))).min_by_key(|(r, _)| *r).map(|(_, o)| o)
}

/// An anonymous memory file (memfd) shared with the compositor.
pub fn memfd(len: u64) -> Result<File, String> {
    // SAFETY: memfd_create with a valid C string; we own the returned fd.
    let fd = unsafe { libc::memfd_create(c"frame".as_ptr(), libc::MFD_CLOEXEC) };
    if fd < 0 {
        return Err(format!("memfd: {}", std::io::Error::last_os_error()));
    }
    // SAFETY: fd is a fresh descriptor nobody else owns.
    let f = File::from(unsafe { OwnedFd::from_raw_fd(fd) });
    f.set_len(len).map_err(|e| format!("memfd: {e}"))?;
    Ok(f)
}

/// A wl_buffer backed by a memfd; pixels move with pread/pwrite (no mmap).
pub struct ShmBuf {
    pub wl: wl_buffer::WlBuffer,
    pub file: File,
    pub stride: u32,
    pub h: u32,
}

impl ShmBuf {
    pub fn new(shm: &wl_shm::WlShm, qh: &QueueHandle<State>, w: u32, h: u32, stride: u32, code: u32) -> Result<ShmBuf, String> {
        Self::with_data(shm, qh, w, h, stride, code, BufTag::Capture)
    }

    pub fn with_data(shm: &wl_shm::WlShm, qh: &QueueHandle<State>, w: u32, h: u32, stride: u32, code: u32, tag: BufTag) -> Result<ShmBuf, String> {
        let len = stride as u64 * h as u64;
        if len == 0 || len > i32::MAX as u64 {
            return Err(format!("bad buffer size {w}x{h}"));
        }
        let file = memfd(len)?;
        let pool = shm.create_pool(file.as_fd(), len as i32, qh, ());
        let format = wl_shm::Format::try_from(code).map_err(|_| format!("pixel format {code:#x}"))?;
        let wl = pool.create_buffer(0, w as i32, h as i32, stride as i32, format, qh, tag);
        pool.destroy();
        Ok(ShmBuf { wl, file, stride, h })
    }

    pub fn read(&self) -> Result<Vec<u8>, String> {
        let mut v = vec![0u8; self.stride as usize * self.h as usize];
        self.file.read_exact_at(&mut v, 0).map_err(|e| format!("reading the capture: {e}"))?;
        Ok(v)
    }
}

/// Who owns a wl_buffer: a capture (no events needed) or the selector's
/// surface N (released = free to draw again).
#[derive(Clone, Copy)]
pub enum BufTag {
    Capture,
    Surface(usize),
}

// ------------------------------------------------------------ dispatch

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<wl_output::WlOutput, usize> for State {
    fn event(st: &mut Self, _: &wl_output::WlOutput, ev: wl_output::Event, i: &usize, _: &Connection, _: &QueueHandle<Self>) {
        let Some(o) = st.outputs.get_mut(*i) else { return };
        match ev {
            wl_output::Event::Geometry { x, y, transform, .. } => {
                o.pos = (x, y);
                o.transform = wenum(transform) & 7;
            }
            wl_output::Event::Mode { flags, width, height, .. } => {
                if wenum(flags) & 1 == 1 {
                    o.mode = (width, height); // the current mode
                }
            }
            wl_output::Event::Scale { factor } => o.scale = factor.max(1),
            wl_output::Event::Name { name } => o.name = name,
            _ => {}
        }
    }
}

impl Dispatch<xo::ZxdgOutputV1, usize> for State {
    fn event(st: &mut Self, _: &xo::ZxdgOutputV1, ev: xo::Event, i: &usize, _: &Connection, _: &QueueHandle<Self>) {
        let Some(o) = st.outputs.get_mut(*i) else { return };
        match ev {
            xo::Event::LogicalPosition { x, y } => o.xdg_pos = Some((x, y)),
            xo::Event::LogicalSize { width, height } => o.xdg_size = Some((width, height)),
            xo::Event::Name { name } if o.name.is_empty() => o.name = name,
            _ => {}
        }
    }
}

impl Dispatch<wl_buffer::WlBuffer, BufTag> for State {
    fn event(st: &mut Self, _: &wl_buffer::WlBuffer, ev: wl_buffer::Event, tag: &BufTag, _: &Connection, _: &QueueHandle<Self>) {
        if let (wl_buffer::Event::Release, BufTag::Surface(i)) = (ev, tag) {
            st.sel.released(*i);
        }
    }
}

impl Dispatch<wframe::ZwlrScreencopyFrameV1, bool> for State {
    fn event(st: &mut Self, _: &wframe::ZwlrScreencopyFrameV1, ev: wframe::Event, v3: &bool, _: &Connection, _: &QueueHandle<Self>) {
        let c = &mut st.cap;
        match ev {
            wframe::Event::Buffer { format, width, height, stride } => {
                c.offers.push((wenum(format), width, height, stride));
                if !v3 {
                    c.constraints_done = true;
                }
            }
            wframe::Event::BufferDone => c.constraints_done = true,
            wframe::Event::Flags { flags } => c.y_invert = wenum(flags) & 1 == 1,
            wframe::Event::Ready { .. } => c.ready = true,
            wframe::Event::Failed => c.failed = Some("the compositor refused".into()),
            _ => {}
        }
    }
}

impl Dispatch<xsession::ExtImageCopyCaptureSessionV1, ()> for State {
    fn event(st: &mut Self, _: &xsession::ExtImageCopyCaptureSessionV1, ev: xsession::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        let c = &mut st.cap;
        match ev {
            xsession::Event::BufferSize { width, height } => c.ext_size = Some((width, height)),
            xsession::Event::ShmFormat { format } => c.offers.push((wenum(format), 0, 0, 0)),
            xsession::Event::Done => c.constraints_done = true,
            xsession::Event::Stopped => c.failed = Some("the capture session stopped".into()),
            _ => {}
        }
    }
}

impl Dispatch<xframe::ExtImageCopyCaptureFrameV1, ()> for State {
    fn event(st: &mut Self, _: &xframe::ExtImageCopyCaptureFrameV1, ev: xframe::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        let c = &mut st.cap;
        match ev {
            xframe::Event::Transform { transform } => c.transform = Some(wenum(transform) & 7),
            xframe::Event::Ready => c.ready = true,
            xframe::Event::Failed { reason } => c.failed = Some(format!("the compositor refused (reason {})", wenum(reason))),
            _ => {}
        }
    }
}

delegate_noop!(State: ignore wl_shm::WlShm);
delegate_noop!(State: ignore wl_shm_pool::WlShmPool);
delegate_noop!(State: ignore xo_mgr::ZxdgOutputManagerV1);
delegate_noop!(State: ignore wmgr::ZwlrScreencopyManagerV1);
delegate_noop!(State: ignore xmgr::ExtImageCopyCaptureManagerV1);
delegate_noop!(State: ignore src_mgr::ExtOutputImageCaptureSourceManagerV1);
delegate_noop!(State: ignore src::ExtImageCaptureSourceV1);

#[cfg(test)]
mod tests {
    #[test]
    fn picks_8bit_first() {
        let offers = [(0x3033_5258, 4, 4, 16), (1, 4, 4, 16), (42, 4, 4, 16)];
        assert_eq!(super::pick(&offers), Some((1, 4, 4, 16)));
        assert_eq!(super::pick(&[(0, 4, 4, 16), (1, 4, 4, 16)]).unwrap().0, 1); // xrgb over argb
        assert_eq!(super::pick(&[(42, 1, 1, 4)]), None);
        assert_eq!(super::pick(&[(0x3033_5258, 1, 1, 4)]).unwrap().0, 0x3033_5258);
    }

    #[test]
    fn memfd_round_trip() {
        use std::os::unix::fs::FileExt;
        let f = super::memfd(16).unwrap();
        f.write_all_at(b"abcd", 12).unwrap();
        let mut b = [0u8; 4];
        f.read_exact_at(&mut b, 12).unwrap();
        assert_eq!(&b, b"abcd");
    }
}
