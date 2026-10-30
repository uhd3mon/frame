//! Our own region selector (slurp's job). The screen is captured first;
//! the selector shows that frozen picture dimmed (#00000055) on every
//! monitor, the user drags a rectangle (white border, undimmed inside),
//! release confirms, Esc or right-click cancels, a click without a drag
//! picks the whole monitor under the pointer. What you see is what you
//! get: nothing that moves after the shortcut ends up in the picture, and
//! the selector's own surfaces or close animations can't either.
//!
//! Surfaces: wlr-layer-shell overlays where the compositor has it (KWin,
//! Sway, Hyprland, other wlroots); otherwise fullscreen xdg-toplevels, one
//! per monitor (GNOME/Mutter has no layer-shell). Software drawing in
//! wl_shm buffers. X11 has its own window (x11.rs) on the same `Sel`.

use crate::pixels::{self, Rect, Shot};
use image::RgbaImage;
use crate::wl::{wenum, BufTag, ShmBuf, State, Wl};
use std::os::unix::fs::FileExt;
use std::time::Duration;
use wayland_client::protocol::{wl_callback, wl_compositor, wl_keyboard, wl_pointer, wl_seat, wl_surface};
use wayland_client::{delegate_noop, Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols::xdg::activation::v1::client::{xdg_activation_token_v1 as atoken, xdg_activation_v1 as activation};
use wayland_protocols::wp::cursor_shape::v1::client::{wp_cursor_shape_device_v1 as cdev, wp_cursor_shape_manager_v1 as cmgr};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1 as layer, zwlr_layer_surface_v1 as lsurf};

/// Dim over everything outside the selection (#00000055, like slurp had).
pub const DIM_ALPHA: u32 = 0x55;
/// Selection border: white at 0xcc, 2 logical px.
pub const BORDER_ALPHA: u32 = 0xcc;
pub const BORDER: i32 = 2;
/// A press and release closer than this (logical px) is a click.
const CLICK: f64 = 3.0;

// ------------------------------------------------------- the selection

/// The drag, in desktop (logical) coordinates. Shared with X11.
#[derive(Default)]
pub struct Sel {
    start: Option<(f64, f64)>,
    pub cur: (f64, f64),
    /// Some(None) = cancelled, Some(Some(r)) = chosen.
    pub done: Option<Option<Rect>>,
    pub monitors: Vec<Rect>,
}

impl Sel {
    pub fn new(monitors: Vec<Rect>) -> Sel {
        Sel { monitors, ..Default::default() }
    }

    pub fn motion(&mut self, x: f64, y: f64) {
        self.cur = (x, y);
    }

    pub fn press(&mut self) {
        if self.done.is_none() {
            self.start = Some(self.cur);
        }
    }

    pub fn release(&mut self) {
        let Some((sx, sy)) = self.start.take() else { return };
        let (x, y) = self.cur;
        if (x - sx).abs() < CLICK && (y - sy).abs() < CLICK {
            let (px, py) = (x.floor() as i32, y.floor() as i32);
            self.done = Some(self.monitors.iter().find(|m| m.contains(px, py)).copied());
        } else {
            self.done = Some(self.rect_between((sx, sy), (x, y)));
        }
    }

    pub fn cancel(&mut self) {
        self.start = None;
        self.done = Some(None);
    }

    /// The rectangle being dragged now (for drawing).
    pub fn current(&self) -> Option<Rect> {
        self.start.and_then(|s| self.rect_between(s, self.cur))
    }

    fn rect_between(&self, a: (f64, f64), b: (f64, f64)) -> Option<Rect> {
        let (x0, y0) = (a.0.min(b.0).round() as i32, a.1.min(b.1).round() as i32);
        let (x1, y1) = (a.0.max(b.0).round() as i32, a.1.max(b.1).round() as i32);
        let r = Rect::new(x0, y0, x1 - x0, y1 - y0);
        (!r.is_empty()).then_some(r)
    }
}

// ------------------------------------------------------------ drawing

/// One monitor's frozen picture at the surface's buffer size: a view
/// into the capture when the densities match (no copy), else a scaled
/// copy (a monitor less dense than the picture it was cut from). Rows
/// are drawn on demand: dimmed outside the selection, plain inside.
pub struct Backdrop<'a> {
    w: u32,
    src: Src<'a>,
}

enum Src<'a> {
    /// The picture and the top-left of our part in it.
    View(&'a RgbaImage, u32, u32),
    Scaled(RgbaImage),
}

impl<'a> Backdrop<'a> {
    /// The part of PARTS (pictures of desktop areas) under AREA (logical),
    /// at W x H pixels.
    pub fn new(parts: &'a [Shot], area: Rect, w: u32, h: u32) -> Backdrop<'a> {
        if let Some(p) = parts.iter().find(|p| p.area.intersect(&area) == Some(area)) {
            let (sx, sy) = p.scale();
            let px = |v: i32, k: f64| (v as f64 * k).round() as u32;
            let (x0, y0) = (px(area.x - p.area.x, sx), px(area.y - p.area.y, sy));
            if (px(area.w, sx), px(area.h, sy)) == (w, h) && x0 + w <= p.img.width() && y0 + h <= p.img.height() {
                return Backdrop { w, src: Src::View(&p.img, x0, y0) };
            }
        }
        let img = pixels::crop_parts(parts, area).unwrap_or_else(|| RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 255])));
        let img = if img.dimensions() == (w, h) { img } else { image::imageops::resize(&img, w, h, image::imageops::FilterType::Triangle) };
        Backdrop { w, src: Src::Scaled(img) }
    }

    /// Row Y of the picture (RGBA, w*4 bytes).
    fn row(&self, y: u32) -> &[u8] {
        let n = self.w as usize * 4;
        let (img, x0, y0) = match &self.src {
            Src::View(img, x0, y0) => (*img, *x0, *y0),
            Src::Scaled(img) => (img, 0, 0),
        };
        let o = (y0 + y) as usize * img.width() as usize * 4 + x0 as usize * 4;
        &img.as_raw()[o..o + n]
    }

    /// Row Y with selection SEL (buffer pixels, border BW px) into OUT
    /// (w*4 bytes, XRGB8888 = B G R X).
    pub fn paint_row(&self, sel: Option<(i32, i32, i32, i32)>, bw: i32, y: u32, out: &mut [u8]) {
        let keep = 255 - DIM_ALPHA;
        let dim = |c: u8| ((c as u32 * keep + 127) / 255) as u8;
        let white = |c: u8| ((c as u32 * (255 - BORDER_ALPHA) + 255 * BORDER_ALPHA + 127) / 255) as u8;
        let (w, y) = (self.w as i32, y as i32);
        // Columns x0..x1 are inside the selection; the border is all of
        // an edge row, else BW px at each side.
        let (x0, x1, edge_row, sx, sw) = match sel {
            Some((sx, sy, sw, sh)) if y >= sy && y < sy + sh => {
                (sx.clamp(0, w), (sx + sw).clamp(0, w), y < sy + bw || y >= sy + sh - bw, sx, sw)
            }
            _ => (0, 0, false, 0, 0),
        };
        for (x, (p, o)) in self.row(y as u32).as_chunks::<4>().0.iter().zip(out.as_chunks_mut::<4>().0).enumerate() {
            let x = x as i32;
            *o = if x < x0 || x >= x1 {
                [dim(p[2]), dim(p[1]), dim(p[0]), 255]
            } else if edge_row || x < sx + bw || x >= sx + sw - bw {
                [white(p[2]), white(p[1]), white(p[0]), 255]
            } else {
                [p[2], p[1], p[0], 255]
            };
        }
    }

    /// Rows Y0..Y1 into OUT (those rows only), for tests.
    #[cfg(test)]
    pub fn paint_rows(&self, sel: Option<(i32, i32, i32, i32)>, bw: i32, y0: u32, y1: u32, out: &mut Vec<u8>) {
        let n = self.w as usize * 4;
        out.clear();
        out.resize((y1 - y0) as usize * n, 0);
        for (i, y) in (y0..y1).enumerate() {
            self.paint_row(sel, bw, y, &mut out[i * n..(i + 1) * n]);
        }
    }
}

// ------------------------------------------------- Wayland surfaces

enum Role {
    Layer(lsurf::ZwlrLayerSurfaceV1),
    Toplevel(xdg_surface::XdgSurface, xdg_toplevel::XdgToplevel),
}

struct Surf {
    wl: wl_surface::WlSurface,
    role: Role,
    /// The output's place on the desktop.
    area: Rect,
    /// Buffer pixels per logical pixel (wl_output.scale).
    scale: i32,
    configured: bool,
    buf: Option<ShmBuf>,
    busy: bool,
    /// The size changed: new buffer and backdrop before the next paint.
    resized: bool,
    /// What is on screen now (buffer px); None before the first paint.
    shown: Option<Option<(i32, i32, i32, i32)>>,
    frame_pending: bool,
}

#[derive(Default)]
pub struct SelState {
    surfs: Vec<Surf>,
    pub sel: Sel,
    /// Surface under the pointer.
    focus: Option<usize>,
    cursor: Option<cdev::WpCursorShapeDeviceV1>,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    error: Option<String>,
    /// xdg-activation: the token asked for, once it arrives.
    token: Option<String>,
}

impl SelState {
    pub fn released(&mut self, i: usize) {
        if let Some(s) = self.surfs.get_mut(i) {
            s.busy = false;
        }
    }
}

/// Let the user pick a rectangle on PARTS (the frozen desktop: one
/// picture per monitor, or one of everything). None = cancelled.
pub fn select(wl: &mut Wl, parts: &[Shot]) -> Result<Option<Rect>, String> {
    let qh = wl.qh();
    let compositor: wl_compositor::WlCompositor = wl.globals.bind(&qh, 1..=4, ()).map_err(|_| "no wl_compositor")?;
    let layer: Option<layer::ZwlrLayerShellV1> = if wl.xdg_only { None } else { wl.globals.bind(&qh, 1..=4, ()).ok() };
    let wm: Option<xdg_wm_base::XdgWmBase> = if layer.is_none() { wl.globals.bind(&qh, 1..=5, ()).ok() } else { None };
    if layer.is_none() && wm.is_none() {
        return Err("the compositor has neither layer-shell nor xdg-shell".into());
    }
    let _seat: wl_seat::WlSeat = wl.globals.bind(&qh, 1..=7, ()).map_err(|_| "no Wayland seat")?;
    let shapes: Option<cmgr::WpCursorShapeManagerV1> = wl.globals.bind(&qh, 1..=1, ()).ok();
    // Fullscreen windows need keyboard focus for Esc; Mutter only gives
    // it to a window that asks with an activation token.
    let act: Option<activation::XdgActivationV1> = if wm.is_some() { wl.globals.bind(&qh, 1..=1, ()).ok() } else { None };

    wl.st.sel = SelState { sel: Sel::new(wl.st.outputs.iter().map(|o| o.logical).collect()), ..Default::default() };
    for (i, o) in wl.st.outputs.iter().enumerate() {
        let s = compositor.create_surface(&qh, ());
        let role = match &layer {
            Some(l) => {
                let ls = l.get_layer_surface(&s, Some(&o.wl), layer::Layer::Overlay, "frame-selection".into(), &qh, i);
                ls.set_anchor(lsurf::Anchor::Top | lsurf::Anchor::Bottom | lsurf::Anchor::Left | lsurf::Anchor::Right);
                ls.set_exclusive_zone(-1);
                ls.set_keyboard_interactivity(lsurf::KeyboardInteractivity::Exclusive);
                Role::Layer(ls)
            }
            None => {
                let wm = wm.as_ref().expect("checked above");
                let xs = wm.get_xdg_surface(&s, &qh, i);
                let tl = xs.get_toplevel(&qh, i);
                tl.set_title("Frame: select an area".into());
                tl.set_app_id("frame".into());
                tl.set_fullscreen(Some(&o.wl));
                Role::Toplevel(xs, tl)
            }
        };
        if s.version() >= 3 {
            s.set_buffer_scale(o.scale);
        }
        s.commit();
        let scale = if s.version() >= 3 { o.scale } else { 1 };
        wl.st.sel.surfs.push(Surf {
            wl: s,
            role,
            area: o.logical,
            scale,
            configured: false,
            buf: None,
            busy: false,
            resized: false,
            shown: None,
            frame_pending: false,
        });
    }
    let r = run(wl, parts, shapes.as_ref(), act.as_ref());
    // Tear down and make sure the compositor has seen it.
    let st = std::mem::take(&mut wl.st.sel);
    for s in st.surfs {
        match s.role {
            Role::Layer(l) => l.destroy(),
            Role::Toplevel(x, t) => {
                t.destroy();
                x.destroy();
            }
        }
        s.wl.destroy();
        if let Some(b) = s.buf {
            b.wl.destroy();
        }
    }
    if let Some(c) = st.cursor {
        c.destroy();
    }
    if let Some(p) = st.pointer {
        if p.version() >= 3 {
            p.release();
        }
    }
    if let Some(k) = st.keyboard {
        if k.version() >= 3 {
            k.release();
        }
    }
    if let Some(l) = layer {
        if l.version() >= 3 {
            l.destroy();
        }
    }
    if let Some(w) = wm {
        w.destroy();
    }
    if let Some(c) = shapes {
        c.destroy();
    }
    if let Some(a) = act {
        a.destroy();
    }
    let _ = wl.roundtrip();
    r
}

fn run(wl: &mut Wl, parts: &[Shot], shapes: Option<&cmgr::WpCursorShapeManagerV1>, act: Option<&activation::XdgActivationV1>) -> Result<Option<Rect>, String> {
    let qh = wl.qh();
    // Pointer and keyboard come with the seat's capabilities.
    wl.roundtrip()?;
    if let (Some(p), Some(m)) = (&wl.st.sel.pointer, shapes) {
        wl.st.sel.cursor = Some(m.get_pointer(p, &qh, ()));
    }
    // A compositor that never shows our surfaces would leave us waiting
    // (with the screen lock held) until the user gives up.
    wl.wait(Duration::from_secs(5), "showing the selector", |s| {
        s.sel.sel.done.is_some() || s.sel.error.is_some() || s.sel.surfs.iter().any(|f| f.configured)
    })?;
    let mut backdrops: Vec<Option<Backdrop>> = (0..wl.st.sel.surfs.len()).map(|_| None).collect();
    let mut activated = act.is_none();
    loop {
        wl.wait(Duration::from_secs(3600), "selection", |s| {
            s.sel.sel.done.is_some() || s.sel.error.is_some() || s.sel.surfs.iter().any(|f| needs_paint(f, &s.sel.sel))
        })?;
        if let Some(e) = wl.st.sel.error.take() {
            return Err(e);
        }
        if let Some(r) = wl.st.sel.sel.done {
            return Ok(r);
        }
        for (i, bd) in backdrops.iter_mut().enumerate() {
            if needs_paint(&wl.st.sel.surfs[i], &wl.st.sel.sel) {
                paint(wl, &qh, i, parts, bd)?;
            }
        }
        if !activated && wl.st.sel.surfs.iter().any(|s| s.shown.is_some()) {
            activated = true;
            activate(wl, act.expect("checked above"));
        }
    }
}

/// Ask for keyboard focus (xdg-activation) for the first shown surface:
/// with the token we were started with (XDG_ACTIVATION_TOKEN, from the
/// desktop's launcher) or else one of our own. Best effort: without it
/// Esc may not reach us, right-click still cancels.
fn activate(wl: &mut Wl, act: &activation::XdgActivationV1) {
    let qh = wl.qh();
    let Some(surf) = wl.st.sel.surfs.iter().find(|s| s.shown.is_some()).map(|s| s.wl.clone()) else { return };
    let token = match wl.activation_token.take() {
        Some(t) => Some(t),
        None => {
            let t = act.get_activation_token(&qh, ());
            t.set_surface(&surf);
            t.set_app_id("frame".into());
            t.commit();
            let _ = wl.wait(Duration::from_secs(1), "activation token", |s| s.sel.token.is_some());
            t.destroy();
            wl.st.sel.token.take()
        }
    };
    if let Some(t) = token {
        act.activate(t, &surf);
    }
}

/// The selection in surface I's buffer pixels.
fn sel_px(s: &Surf, sel: &Sel) -> Option<(i32, i32, i32, i32)> {
    let r = sel.current()?;
    let k = s.scale;
    Some(((r.x - s.area.x) * k, (r.y - s.area.y) * k, r.w * k, r.h * k))
}

fn needs_paint(s: &Surf, sel: &Sel) -> bool {
    s.configured && !s.busy && !s.frame_pending && s.shown != Some(sel_px(s, sel))
}

/// Rows drawn per write into the shared buffer.
const BAND: u32 = 64;

fn paint<'a>(wl: &mut Wl, qh: &QueueHandle<State>, i: usize, parts: &'a [Shot], bd: &mut Option<Backdrop<'a>>) -> Result<(), String> {
    let shm = wl.shm()?;
    let st = &mut wl.st.sel;
    let want = sel_px(&st.surfs[i], &st.sel);
    let s = &mut st.surfs[i];
    let (w, h) = ((s.area.w * s.scale) as u32, (s.area.h * s.scale) as u32);
    if s.resized || bd.is_none() {
        if let Some(old) = s.buf.take() {
            old.wl.destroy();
        }
        *bd = None;
        s.resized = false;
        s.shown = None;
        s.buf = Some(ShmBuf::with_data(&shm, qh, w, h, w * 4, 1, BufTag::Surface(i))?);
        *bd = Some(Backdrop::new(parts, s.area, w, h));
    }
    let (bd, buf) = (bd.as_ref().expect("set"), s.buf.as_ref().expect("set"));
    let bw = BORDER * s.scale;
    // Repaint only the rows that changed: old and new selection.
    let band = |r: Option<(i32, i32, i32, i32)>| r.map(|(_, y, _, hh)| (y.clamp(0, h as i32) as u32, (y + hh).clamp(0, h as i32) as u32));
    let (y0, y1) = match s.shown {
        None => (0, h),
        Some(old) => match (band(old), band(want)) {
            (Some(a), Some(b)) => (a.0.min(b.0), a.1.max(b.1)),
            (Some(a), None) | (None, Some(a)) => a,
            (None, None) => (0, 0),
        },
    };
    if y1 > y0 {
        // Straight into the shared memory, a band of rows at a time.
        let n = w as usize * 4;
        let mut rows = vec![0u8; n * BAND.min(y1 - y0) as usize];
        let mut y = y0;
        while y < y1 {
            let k = BAND.min(y1 - y);
            for r in 0..k {
                bd.paint_row(want, bw, y + r, &mut rows[r as usize * n..(r as usize + 1) * n]);
            }
            buf.file.write_all_at(&rows[..k as usize * n], y as u64 * n as u64).map_err(|e| format!("drawing: {e}"))?;
            y += k;
        }
        s.wl.attach(Some(&buf.wl), 0, 0);
        if s.wl.version() >= 4 {
            s.wl.damage_buffer(0, y0 as i32, w as i32, (y1 - y0) as i32);
        } else {
            s.wl.damage(0, 0, i32::MAX, i32::MAX);
        }
        s.wl.frame(qh, i);
        s.frame_pending = true;
        s.busy = true;
        s.wl.commit();
    }
    s.shown = Some(want);
    Ok(())
}

// ------------------------------------------------------------ dispatch

impl Dispatch<lsurf::ZwlrLayerSurfaceV1, usize> for State {
    fn event(st: &mut Self, ls: &lsurf::ZwlrLayerSurfaceV1, ev: lsurf::Event, i: &usize, _: &Connection, _: &QueueHandle<Self>) {
        match ev {
            lsurf::Event::Configure { serial, width, height } => {
                ls.ack_configure(serial);
                if let Some(s) = st.sel.surfs.get_mut(*i) {
                    let size = (width as i32, height as i32);
                    if width > 0 && height > 0 && size != (s.area.w, s.area.h) {
                        (s.area.w, s.area.h) = size;
                        s.resized = true;
                    }
                    s.configured = true;
                }
            }
            lsurf::Event::Closed => st.sel.sel.cancel(),
            _ => {}
        }
    }
}

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for State {
    fn event(_: &mut Self, wm: &xdg_wm_base::XdgWmBase, ev: xdg_wm_base::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let xdg_wm_base::Event::Ping { serial } = ev {
            wm.pong(serial);
        }
    }
}

impl Dispatch<xdg_surface::XdgSurface, usize> for State {
    fn event(st: &mut Self, xs: &xdg_surface::XdgSurface, ev: xdg_surface::Event, i: &usize, _: &Connection, _: &QueueHandle<Self>) {
        if let xdg_surface::Event::Configure { serial } = ev {
            xs.ack_configure(serial);
            if let Some(s) = st.sel.surfs.get_mut(*i) {
                s.configured = true;
            }
        }
    }
}

impl Dispatch<xdg_toplevel::XdgToplevel, usize> for State {
    fn event(st: &mut Self, _: &xdg_toplevel::XdgToplevel, ev: xdg_toplevel::Event, i: &usize, _: &Connection, _: &QueueHandle<Self>) {
        match ev {
            xdg_toplevel::Event::Configure { width, height, .. } => {
                // Fullscreen: the monitor's size. A different size would
                // mean our picture no longer lines up; keep the monitor's.
                if let Some(s) = st.sel.surfs.get_mut(*i) {
                    if width > 0 && height > 0 && (width, height) != (s.area.w, s.area.h) {
                        s.area.w = width;
                        s.area.h = height;
                        s.resized = true;
                    }
                }
            }
            xdg_toplevel::Event::Close => st.sel.sel.cancel(),
            _ => {}
        }
    }
}

impl Dispatch<wl_callback::WlCallback, usize> for State {
    fn event(st: &mut Self, _: &wl_callback::WlCallback, ev: wl_callback::Event, i: &usize, _: &Connection, _: &QueueHandle<Self>) {
        if let wl_callback::Event::Done { .. } = ev {
            if let Some(s) = st.sel.surfs.get_mut(*i) {
                s.frame_pending = false;
                // Compositors that keep the buffer until the next attach
                // would stall us; a shown frame is enough to draw again.
                s.busy = false;
            }
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(st: &mut Self, seat: &wl_seat::WlSeat, ev: wl_seat::Event, _: &(), _: &Connection, qh: &QueueHandle<Self>) {
        if let wl_seat::Event::Capabilities { capabilities } = ev {
            let caps = wenum(capabilities);
            if caps & 1 != 0 && st.sel.pointer.is_none() {
                st.sel.pointer = Some(seat.get_pointer(qh, ()));
            }
            if caps & 2 != 0 && st.sel.keyboard.is_none() {
                st.sel.keyboard = Some(seat.get_keyboard(qh, ()));
            }
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for State {
    fn event(st: &mut Self, _: &wl_pointer::WlPointer, ev: wl_pointer::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        let s = &mut st.sel;
        let at = |s: &SelState, i: Option<usize>, x: f64, y: f64| {
            i.and_then(|i| s.surfs.get(i)).map(|f| (f.area.x as f64 + x, f.area.y as f64 + y))
        };
        match ev {
            wl_pointer::Event::Enter { serial, surface, surface_x, surface_y } => {
                s.focus = s.surfs.iter().position(|f| f.wl == surface);
                if let Some(c) = &s.cursor {
                    c.set_shape(serial, cdev::Shape::Crosshair);
                }
                if let Some(p) = at(s, s.focus, surface_x, surface_y) {
                    s.sel.motion(p.0, p.1);
                }
            }
            wl_pointer::Event::Leave { .. } => {} // a drag keeps its surface (implicit grab)
            wl_pointer::Event::Motion { surface_x, surface_y, .. } => {
                if let Some(p) = at(s, s.focus, surface_x, surface_y) {
                    s.sel.motion(p.0, p.1);
                }
            }
            wl_pointer::Event::Button { button, state, .. } => {
                let pressed = wenum(state) == 1;
                match (button, pressed) {
                    (0x110, true) => s.sel.press(),    // BTN_LEFT
                    (0x110, false) => s.sel.release(), // BTN_LEFT
                    (0x111, true) => s.sel.cancel(),   // BTN_RIGHT
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for State {
    fn event(st: &mut Self, _: &wl_keyboard::WlKeyboard, ev: wl_keyboard::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        // evdev KEY_ESC = 1. The keymap fd (Keymap event) is closed on drop.
        if let wl_keyboard::Event::Key { key: 1, state, .. } = ev {
            if wenum(state) == 1 {
                st.sel.sel.cancel();
            }
        }
    }
}

delegate_noop!(State: ignore wl_compositor::WlCompositor);
delegate_noop!(State: ignore wl_surface::WlSurface);
delegate_noop!(State: ignore layer::ZwlrLayerShellV1);
delegate_noop!(State: ignore cmgr::WpCursorShapeManagerV1);
delegate_noop!(State: ignore cdev::WpCursorShapeDeviceV1);
delegate_noop!(State: ignore activation::XdgActivationV1);

impl Dispatch<atoken::XdgActivationTokenV1, ()> for State {
    fn event(st: &mut Self, _: &atoken::XdgActivationTokenV1, ev: atoken::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let atoken::Event::Done { token } = ev {
            st.sel.token = Some(token);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mons() -> Vec<Rect> {
        vec![Rect::new(0, 0, 100, 50), Rect::new(100, 0, 100, 50)]
    }

    #[test]
    fn drag_click_cancel() {
        let mut s = Sel::new(mons());
        s.motion(80.4, 40.0);
        s.press();
        s.motion(20.0, 10.6);
        assert_eq!(s.current(), Some(Rect::new(20, 11, 60, 29)));
        s.release();
        assert_eq!(s.done, Some(Some(Rect::new(20, 11, 60, 29))));

        // A click picks the monitor under the pointer.
        let mut s = Sel::new(mons());
        s.motion(150.0, 20.0);
        s.press();
        s.motion(151.0, 21.0);
        s.release();
        assert_eq!(s.done, Some(Some(Rect::new(100, 0, 100, 50))));

        // Right-click / Esc / closed surface; release without press is ignored.
        let mut s = Sel::new(mons());
        s.release();
        assert_eq!(s.done, None);
        s.press();
        s.cancel();
        assert_eq!((s.done, s.current()), (Some(None), None));
        s.press(); // nothing after the end
        assert_eq!(s.current(), None);

        // A drag across monitors spans both.
        let mut s = Sel::new(mons());
        s.motion(90.0, 5.0);
        s.press();
        s.motion(130.0, 45.0);
        s.release();
        assert_eq!(s.done, Some(Some(Rect::new(90, 5, 40, 40))));
    }

    #[test]
    fn backdrop_paint() {
        let img = image::RgbaImage::from_pixel(4, 4, image::Rgba([200, 100, 50, 255]));
        let shot = [Shot { img, area: Rect::new(0, 0, 4, 4) }];
        let b = Backdrop::new(&shot, Rect::new(0, 0, 4, 4), 4, 4);
        assert!(matches!(b.src, Src::View(..)), "same density: no copy");
        let mut out = Vec::new();
        b.paint_rows(None, 1, 0, 4, &mut out);
        assert_eq!(out.len(), 64);
        assert!(out.as_chunks::<4>().0.iter().all(|p| p == &[33, 67, 133, 255])); // B G R X * (255-0x55)/255
        // 4x4 selection at (0,0) with a 1 px border: the middle 2x2 is
        // the plain picture, the ring is white-ish, rows 1..3 only.
        b.paint_rows(Some((0, 0, 4, 4)), 1, 1, 3, &mut out);
        assert_eq!(out.len(), 2 * 16);
        let px = |x: usize, y: usize| &out[y * 16 + x * 4..y * 16 + x * 4 + 4];
        assert_eq!(px(1, 0), &[50, 100, 200, 255]);
        assert_eq!(px(0, 0), &[214, 224, 244, 255]); // 0.8 white over the picture
        // A selection partly off the surface is clipped, not a panic.
        b.paint_rows(Some((-10, -10, 12, 12)), 2, 0, 4, &mut out);
        assert_eq!(out.len(), 4 * 16);
    }

    #[test]
    fn backdrop_scales() {
        let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 30, 255]));
        let shot = [Shot { img, area: Rect::new(0, 0, 2, 2) }];
        let b = Backdrop::new(&shot, Rect::new(0, 0, 2, 2), 4, 4);
        assert!(matches!(&b.src, Src::Scaled(i) if i.dimensions() == (4, 4)));
        let mut out = Vec::new();
        b.paint_rows(Some((0, 0, 4, 4)), 0, 3, 4, &mut out);
        assert_eq!(&out[..4], &[30, 20, 10, 255]);
    }

    /// The right monitor of a stitched desktop is a view at an offset.
    #[test]
    fn backdrop_view_offset() {
        let img = image::RgbaImage::from_fn(4, 2, |x, _| image::Rgba([if x < 2 { 1 } else { 2 }, 0, 0, 255]));
        let shot = [Shot { img, area: Rect::new(0, 0, 4, 2) }];
        let b = Backdrop::new(&shot, Rect::new(2, 0, 2, 2), 2, 2);
        assert!(matches!(b.src, Src::View(_, 2, 0)));
        let mut out = Vec::new();
        b.paint_rows(Some((0, 0, 2, 2)), 0, 0, 2, &mut out);
        assert!(out.as_chunks::<4>().0.iter().all(|p| p[2] == 2), "{out:?}");
    }
}
