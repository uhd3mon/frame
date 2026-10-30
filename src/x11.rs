//! X11 without helper programs (no maim, import, xdotool or xclip), over
//! x11rb's pure-Rust connection: the screen via GetImage on the root
//! window (monitors from RandR), the active window via
//! _NET_ACTIVE_WINDOW and its frame extents, the region selector as an
//! override-redirect window showing the frozen screen, and the clipboard
//! as our own CLIPBOARD selection owner (`frame _clip FILE MIME x11`).

use crate::pixels::{self, Fmt, Rect, Shot};
use crate::select::{Sel, BORDER, DIM_ALPHA};
use std::time::{Duration, Instant};
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::xproto::{self, AtomEnum, ConnectionExt as _, EventMask, ImageFormat, ImageOrder, PropMode, Window};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

fn xerr(e: impl std::fmt::Display) -> String {
    format!("X11: {e}")
}

pub struct X {
    pub conn: RustConnection,
    pub root: Window,
    pub w: u16,
    pub h: u16,
    depth: u8,
}

impl X {
    /// DISPLAY = None: the process's $DISPLAY.
    pub fn connect(display: Option<&str>) -> Result<X, String> {
        let (conn, n) = x11rb::connect(display).map_err(xerr)?;
        let s = &conn.setup().roots[n];
        let (root, w, h, depth) = (s.root, s.width_in_pixels, s.height_in_pixels, s.root_depth);
        Ok(X { conn, root, w, h, depth })
    }

    fn atom(&self, name: &str) -> Result<u32, String> {
        Ok(self.conn.intern_atom(false, name.as_bytes()).map_err(xerr)?.reply().map_err(xerr)?.atom)
    }

    /// How a ZPixmap of DEPTH is laid out; only 32 bits per pixel.
    fn fmt(&self, depth: u8) -> Result<(Fmt, bool), String> {
        let bpp = self.conn.setup().pixmap_formats.iter().find(|f| f.depth == depth).map(|f| f.bits_per_pixel);
        if bpp != Some(32) || !(depth == 24 || depth == 32) {
            return Err(format!("unsupported X11 pixel format (depth {depth}, {bpp:?} bits per pixel)"));
        }
        Ok((Fmt::Xrgb8888 { alpha: false }, self.conn.setup().image_byte_order == ImageOrder::MSB_FIRST))
    }

    /// The whole screen.
    pub fn capture(&self) -> Result<Shot, String> {
        let (w, h) = (self.w as u32, self.h as u32);
        let row = w as usize * 4;
        let strip = ((8 << 20) / row).clamp(1, h as usize) as u32;
        let mut data = Vec::with_capacity(row * h as usize);
        let mut fmt = None;
        let mut y = 0;
        while y < h {
            let n = strip.min(h - y);
            let r = self.conn.get_image(ImageFormat::Z_PIXMAP, self.root, 0, y as i16, w as u16, n as u16, !0).map_err(xerr)?.reply().map_err(xerr)?;
            if fmt.is_none() {
                fmt = Some(self.fmt(r.depth)?);
            }
            if r.data.len() < row * n as usize {
                return Err("X11 sent a short image".into());
            }
            data.extend_from_slice(&r.data[..row * n as usize]);
            y += n;
        }
        let (fmt, msb) = fmt.ok_or("empty screen")?;
        if msb {
            for p in data.as_chunks_mut::<4>().0 {
                p.reverse();
            }
        }
        let img = pixels::from_raw(w, h, row, fmt, &data, false)?;
        Ok(Shot { img, area: Rect::new(0, 0, w as i32, h as i32) })
    }

    pub fn monitors(&self) -> Vec<Rect> {
        let m = self.conn.randr_get_monitors(self.root, true).ok().and_then(|c| c.reply().ok());
        let v: Vec<Rect> = m.map(|m| m.monitors.iter().map(|m| Rect::new(m.x as i32, m.y as i32, m.width as i32, m.height as i32)).collect()).unwrap_or_default();
        if v.is_empty() {
            vec![Rect::new(0, 0, self.w as i32, self.h as i32)]
        } else {
            v
        }
    }

    pub fn pointer(&self) -> Option<(i32, i32)> {
        let p = self.conn.query_pointer(self.root).ok()?.reply().ok()?;
        Some((p.root_x as i32, p.root_y as i32))
    }

    /// The monitor under the pointer (else the first).
    pub fn current_monitor(&self) -> Rect {
        let ms = self.monitors();
        let at = self.pointer();
        at.and_then(|(x, y)| ms.iter().find(|m| m.contains(x, y)).copied()).unwrap_or(ms[0])
    }

    fn prop32(&self, win: Window, name: &str) -> Option<Vec<u32>> {
        let a = self.atom(name).ok()?;
        let r = self.conn.get_property(false, win, a, AtomEnum::ANY, 0, 64).ok()?.reply().ok()?;
        let v: Vec<u32> = r.value32()?.collect();
        (!v.is_empty()).then_some(v)
    }

    /// The active window with its title bar and borders.
    pub fn active_window(&self) -> Result<Rect, String> {
        let win = match self.prop32(self.root, "_NET_ACTIVE_WINDOW").and_then(|v| v.first().copied()).filter(|&w| w != 0) {
            Some(w) => w,
            None => self.conn.get_input_focus().map_err(xerr)?.reply().map_err(xerr)?.focus,
        };
        if win == self.root || win <= 1 {
            return Err("no active window".into());
        }
        let g = self.conn.get_geometry(win).map_err(xerr)?.reply().map_err(|_| "no active window".to_string())?;
        let t = self.conn.translate_coordinates(win, self.root, 0, 0).map_err(xerr)?.reply().map_err(xerr)?;
        let ext = |name| self.prop32(win, name).filter(|v| v.len() == 4).map(|v| [v[0] as i32, v[1] as i32, v[2] as i32, v[3] as i32]);
        Ok(window_rect(
            Rect::new(t.dst_x as i32, t.dst_y as i32, g.width as i32, g.height as i32),
            ext("_NET_FRAME_EXTENTS"),
            ext("_GTK_FRAME_EXTENTS"),
        ))
    }
}

/// A client rectangle grown by the window manager's frame (left, right,
/// top, bottom) and shrunk by a client-side shadow (_GTK_FRAME_EXTENTS).
fn window_rect(r: Rect, frame: Option<[i32; 4]>, gtk: Option<[i32; 4]>) -> Rect {
    let [l, ri, t, b] = frame.unwrap_or_default();
    let [gl, gr, gt, gb] = gtk.unwrap_or_default();
    Rect::new(r.x - l + gl, r.y - t + gt, r.w + l + ri - gl - gr, r.h + t + b - gt - gb)
}

// ------------------------------------------------------------ selector

/// Let the user pick a rectangle on SHOT (the frozen screen).
pub fn select(x: &X, shot: &Shot) -> Result<Option<Rect>, String> {
    let c = &x.conn;
    let (w, h) = (x.w, x.h);
    let (_, msb) = x.fmt(x.depth)?;
    // Server-side pictures: plain and dimmed, so drawing is only copies.
    let to_x = |dim: bool| -> Vec<u8> {
        let keep = 255 - DIM_ALPHA;
        let d = |c: u8| if dim { ((c as u32 * keep + 127) / 255) as u8 } else { c };
        shot.img
            .as_raw()
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| if msb { [0, d(p[0]), d(p[1]), d(p[2])] } else { [d(p[2]), d(p[1]), d(p[0]), 0] })
            .collect()
    };
    let gc = c.generate_id().map_err(xerr)?;
    c.create_gc(gc, x.root, &xproto::CreateGCAux::new().foreground(0xffffff).graphics_exposures(0)).map_err(xerr)?;
    let mut pix = [0u32; 2];
    for (i, dim) in [(0, false), (1, true)] {
        pix[i] = c.generate_id().map_err(xerr)?;
        c.create_pixmap(x.depth, pix[i], x.root, w, h).map_err(xerr)?;
        let data = to_x(dim);
        let row = w as usize * 4;
        let rows = ((c.maximum_request_bytes().saturating_sub(64)) / row).clamp(1, h as usize);
        for (n, chunk) in data.chunks(row * rows).enumerate() {
            let y = (n * rows) as i16;
            let ch = (chunk.len() / row) as u16;
            c.put_image(ImageFormat::Z_PIXMAP, pix[i], gc, w, ch, 0, y, 0, x.depth, chunk).map_err(xerr)?;
        }
    }
    let (bright, dim) = (pix[0], pix[1]);
    let font = c.generate_id().map_err(xerr)?;
    c.open_font(font, b"cursor").map_err(xerr)?;
    let cursor = c.generate_id().map_err(xerr)?;
    c.create_glyph_cursor(cursor, font, font, 34, 35, 0, 0, 0, 0xffff, 0xffff, 0xffff).map_err(xerr)?; // XC_crosshair
    let win = c.generate_id().map_err(xerr)?;
    let mask = EventMask::EXPOSURE | EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION | EventMask::KEY_PRESS | EventMask::STRUCTURE_NOTIFY;
    let aux = xproto::CreateWindowAux::new().override_redirect(1).background_pixmap(dim).event_mask(mask).cursor(cursor);
    c.create_window(x.depth, win, x.root, 0, 0, w, h, 0, xproto::WindowClass::INPUT_OUTPUT, 0, &aux).map_err(xerr)?;
    c.map_window(win).map_err(xerr)?;
    c.flush().map_err(xerr)?;
    let r = run_select(x, win, gc, bright, dim, cursor);
    let _ = c.ungrab_pointer(x11rb::CURRENT_TIME);
    let _ = c.ungrab_keyboard(x11rb::CURRENT_TIME);
    let _ = c.destroy_window(win);
    let _ = c.free_pixmap(bright);
    let _ = c.free_pixmap(dim);
    let _ = c.free_gc(gc);
    let _ = c.free_cursor(cursor);
    let _ = c.close_font(font);
    let _ = c.get_input_focus().map(|r| r.reply()); // round trip: the window is gone
    r
}

fn run_select(x: &X, win: Window, gc: u32, bright: u32, dim: u32, cursor: u32) -> Result<Option<Rect>, String> {
    let c = &x.conn;
    let esc = escape_keycodes(x);
    // Wait until mapped, then grab (the shortcut's own grab may still be
    // held for a moment, so retry like other screenshot tools do).
    let end = Instant::now() + Duration::from_secs(2);
    let mut grabbed = (false, false);
    while Instant::now() < end && !(grabbed.0 && grabbed.1) {
        if !grabbed.0 {
            let m = EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION;
            let r = c.grab_pointer(false, win, m, xproto::GrabMode::ASYNC, xproto::GrabMode::ASYNC, x11rb::NONE, cursor, x11rb::CURRENT_TIME);
            grabbed.0 = r.ok().and_then(|r| r.reply().ok()).is_some_and(|r| r.status == xproto::GrabStatus::SUCCESS);
        }
        if !grabbed.1 {
            let r = c.grab_keyboard(false, win, x11rb::CURRENT_TIME, xproto::GrabMode::ASYNC, xproto::GrabMode::ASYNC);
            grabbed.1 = r.ok().and_then(|r| r.reply().ok()).is_some_and(|r| r.status == xproto::GrabStatus::SUCCESS);
        }
        if !(grabbed.0 && grabbed.1) {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    if !grabbed.0 {
        return Err("could not grab the pointer (another program holds it)".into());
    }
    let mut sel = Sel::new(x.monitors());
    if let Some((px, py)) = x.pointer() {
        sel.motion(px as f64, py as f64);
    }
    let mut shown: Option<Rect> = None;
    loop {
        let mut ev = Some(c.wait_for_event().map_err(xerr)?);
        while let Some(e) = ev {
            match e {
                Event::MotionNotify(m) => sel.motion(m.root_x as f64, m.root_y as f64),
                Event::ButtonPress(b) if b.detail == 1 => {
                    sel.motion(b.root_x as f64, b.root_y as f64);
                    sel.press();
                }
                Event::ButtonPress(b) if b.detail == 3 => sel.cancel(),
                Event::ButtonRelease(b) if b.detail == 1 => {
                    sel.motion(b.root_x as f64, b.root_y as f64);
                    sel.release();
                }
                Event::KeyPress(k) if esc.contains(&k.detail) => sel.cancel(),
                Event::Expose(_) => shown = shown.map(|_| Rect::new(0, 0, x.w as i32, x.h as i32)), // repaint below
                Event::UnmapNotify(_) | Event::DestroyNotify(_) => sel.cancel(),
                _ => {}
            }
            ev = c.poll_for_event().map_err(xerr)?;
        }
        if let Some(r) = sel.done {
            return Ok(r);
        }
        let now = sel.current();
        if now != shown {
            let grow = |r: Rect| Rect::new(r.x - 1, r.y - 1, r.w + 2, r.h + 2);
            if let Some(o) = shown {
                let o = grow(o);
                c.copy_area(dim, win, gc, o.x as i16, o.y as i16, o.x as i16, o.y as i16, o.w as u16, o.h as u16).map_err(xerr)?;
            }
            if let Some(n) = now {
                c.copy_area(bright, win, gc, n.x as i16, n.y as i16, n.x as i16, n.y as i16, n.w as u16, n.h as u16).map_err(xerr)?;
                let b = BORDER.min(n.w / 2).min(n.h / 2).max(1);
                let rs = [
                    xproto::Rectangle { x: n.x as i16, y: n.y as i16, width: n.w as u16, height: b as u16 },
                    xproto::Rectangle { x: n.x as i16, y: (n.bottom() - b) as i16, width: n.w as u16, height: b as u16 },
                    xproto::Rectangle { x: n.x as i16, y: n.y as i16, width: b as u16, height: n.h as u16 },
                    xproto::Rectangle { x: (n.right() - b) as i16, y: n.y as i16, width: b as u16, height: n.h as u16 },
                ];
                c.poly_fill_rectangle(win, gc, &rs).map_err(xerr)?;
            }
            c.flush().map_err(xerr)?;
            shown = now;
        }
    }
}

fn escape_keycodes(x: &X) -> Vec<u8> {
    let s = x.conn.setup();
    let (min, max) = (s.min_keycode, s.max_keycode);
    let Ok(Ok(m)) = x.conn.get_keyboard_mapping(min, max - min + 1).map(|c| c.reply()) else { return vec![9] };
    let per = m.keysyms_per_keycode.max(1) as usize;
    let v: Vec<u8> = m.keysyms.chunks(per).enumerate().filter(|(_, ks)| ks.contains(&0xff1b)).map(|(i, _)| min + i as u8).collect();
    if v.is_empty() {
        vec![9]
    } else {
        v
    }
}

// ----------------------------------------------------------- clipboard

/// Bigger answers go in INCR chunks of at most this size (ICCCM), and
/// never more than one ChangeProperty request can carry.
const CHUNK: usize = 256 * 1024;

struct Incr {
    requestor: Window,
    property: u32,
    kind: u32,
    sent: usize,
}

/// `frame _clip FILE MIME x11`: own CLIPBOARD, print "ok", answer
/// requests until another program takes it.
pub fn serve_clipboard(data: &[u8], mime: &str, ready: impl FnOnce()) -> Result<(), String> {
    let x = X::connect(None)?;
    let c = &x.conn;
    let a = |n: &str| x.atom(n);
    let (clipboard, targets, timestamp, incr, mime_a) = (a("CLIPBOARD")?, a("TARGETS")?, a("TIMESTAMP")?, a("INCR")?, a(mime)?);
    // ChangeProperty's own header is 24 bytes; keep a margin.
    let chunk = CHUNK.min(c.maximum_request_bytes().saturating_sub(32)).max(1);
    let win = c.generate_id().map_err(xerr)?;
    let aux = xproto::CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE);
    c.create_window(0, win, x.root, -10, -10, 1, 1, 0, xproto::WindowClass::INPUT_ONLY, 0, &aux).map_err(xerr)?;
    // A real timestamp for the ownership (ICCCM): touch a property and
    // read the server time from the notification.
    c.change_property8(PropMode::APPEND, win, AtomEnum::WM_NAME, AtomEnum::STRING, b"").map_err(xerr)?;
    c.flush().map_err(xerr)?;
    let time = loop {
        if let Event::PropertyNotify(p) = c.wait_for_event().map_err(xerr)? {
            break p.time;
        }
    };
    c.set_selection_owner(win, clipboard, time).map_err(xerr)?;
    let owner = c.get_selection_owner(clipboard).map_err(xerr)?.reply().map_err(xerr)?.owner;
    if owner != win {
        return Err("another program kept the clipboard".into());
    }
    ready();
    let mut incrs: Vec<Incr> = Vec::new();
    loop {
        match c.wait_for_event().map_err(xerr)? {
            Event::SelectionRequest(r) => {
                let prop = if r.property == x11rb::NONE { r.target } else { r.property };
                let ok = if r.selection != clipboard {
                    false
                } else if r.target == targets {
                    c.change_property32(PropMode::REPLACE, r.requestor, prop, AtomEnum::ATOM, &[targets, timestamp, mime_a]).is_ok()
                } else if r.target == timestamp {
                    c.change_property32(PropMode::REPLACE, r.requestor, prop, AtomEnum::INTEGER, &[time]).is_ok()
                } else if r.target == mime_a && data.len() <= chunk {
                    c.change_property8(PropMode::REPLACE, r.requestor, prop, mime_a, data).is_ok()
                } else if r.target == mime_a {
                    let aux = xproto::ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE);
                    let _ = c.change_window_attributes(r.requestor, &aux);
                    incrs.retain(|i| !(i.requestor == r.requestor && i.property == prop));
                    incrs.push(Incr { requestor: r.requestor, property: prop, kind: mime_a, sent: 0 });
                    c.change_property32(PropMode::REPLACE, r.requestor, prop, incr, &[data.len() as u32]).is_ok()
                } else {
                    false
                };
                let ev = xproto::SelectionNotifyEvent {
                    response_type: xproto::SELECTION_NOTIFY_EVENT,
                    sequence: 0,
                    time: r.time,
                    requestor: r.requestor,
                    selection: r.selection,
                    target: r.target,
                    property: if ok { prop } else { x11rb::NONE },
                };
                let _ = c.send_event(false, r.requestor, EventMask::NO_EVENT, ev);
                let _ = c.flush();
            }
            Event::PropertyNotify(p) if p.state == xproto::Property::DELETE => {
                if let Some(i) = incrs.iter().position(|t| t.requestor == p.window && t.property == p.atom) {
                    let t = &mut incrs[i];
                    let part = &data[t.sent..(t.sent + chunk).min(data.len())];
                    let _ = c.change_property8(PropMode::REPLACE, t.requestor, t.property, t.kind, part);
                    t.sent += part.len();
                    if part.is_empty() {
                        incrs.remove(i); // the empty chunk ends the transfer
                    }
                    let _ = c.flush();
                }
            }
            Event::SelectionClear(_) => return Ok(()),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_extents() {
        let r = Rect::new(100, 100, 400, 300);
        assert_eq!(window_rect(r, None, None), r);
        assert_eq!(window_rect(r, Some([2, 2, 30, 2]), None), Rect::new(98, 70, 404, 332));
        // GTK client-side decorations: the shadow is not part of the window.
        assert_eq!(window_rect(r, None, Some([20, 20, 10, 30])), Rect::new(120, 110, 360, 260));
    }
}
