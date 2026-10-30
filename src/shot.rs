//! `frame region|window|monitor|all`: take the screenshot, save it to
//! <save_dir>/frame/images, copy it to the clipboard, show a toast.
//! Everything happens in this binary: capture (KWin, screencopy, portal,
//! X11), area selection, encoding; only the clipboard and the toast are
//! served by detached `frame _clip` / `frame _toast` processes.

use crate::ctx::Ctx;
use crate::detect::{self, Capture, Clipboard, Desktop};
use crate::pixels::{self, Rect, Shot};
use crate::wl::Wl;
use crate::x11::{self, X};
use crate::{grab, ipc, portal, select};
use image::RgbaImage;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

/// Why a screenshot didn't happen. Failed is shown as a notification too.
/// Busy = another screenshot is under way (quiet, exit 0).
pub enum Stop {
    Cancelled,
    Busy,
    Failed(String),
}

fn failed(e: impl Into<String>) -> Stop {
    Stop::Failed(e.into())
}

/// Local time as 2026-10-29_14-03-59.
fn stamp() -> String {
    // SAFETY: localtime_r and strftime write only into our buffers.
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        let mut buf = [0u8; 32];
        let n = libc::strftime(buf.as_mut_ptr().cast(), buf.len(), c"%Y-%m-%d_%H-%M-%S".as_ptr(), &tm);
        String::from_utf8_lossy(&buf[..n]).into_owned()
    }
}

/// An unused name in DIR for ACTION.EXT (adds -2, -3 within one second).
fn new_file(dir: &Path, action: &str, ext: &str) -> PathBuf {
    let base = format!("{}_{action}", stamp());
    let mut f = dir.join(format!("{base}.{ext}"));
    let mut i = 2;
    while f.symlink_metadata().is_ok() {
        f = dir.join(format!("{base}-{i}.{ext}"));
        i += 1;
    }
    f
}

pub fn mime_of(f: &Path) -> &'static str {
    match f.extension().and_then(|e| e.to_str()) {
        Some("jpg") => "image/jpeg",
        Some("webp") => "image/webp",
        _ => "image/png",
    }
}

/// The Wayland display named by our environment.
fn wayland(ctx: &Ctx) -> Result<Wl, Stop> {
    let d = ctx.var("WAYLAND_DISPLAY");
    let d = if d.is_empty() { "wayland-0".to_string() } else { d };
    let path = if d.starts_with('/') { PathBuf::from(&d) } else { ctx.runtime_dir().ok_or_else(|| failed("XDG_RUNTIME_DIR is not set"))?.join(&d) };
    let mut wl = Wl::connect_to(&path).map_err(Stop::Failed)?;
    wl.wlr_only = ctx.var("FRAME_SCREENCOPY") == "wlr";
    wl.xdg_only = ctx.var("FRAME_SELECT") == "xdg";
    wl.activation_token = Some(ctx.var("XDG_ACTIVATION_TOKEN")).filter(|t| !t.is_empty());
    if ctx.var("FRAME_DEBUG") == "1" {
        ctx.warn(&format!("capture protocol: {}", wl.protocol()));
    }
    Ok(wl)
}

fn x11(ctx: &Ctx) -> Result<X, Stop> {
    let d = ctx.var("DISPLAY");
    X::connect(if d.is_empty() { None } else { Some(&d) }).map_err(Stop::Failed)
}

/// Let the user pick an area of SHOT; the picked part.
fn pick(sel: Result<Option<Rect>, String>, shot: &Shot) -> Result<RgbaImage, Stop> {
    match sel.map_err(|e| failed(format!("area selection: {e}")))? {
        Some(r) => shot.crop(r).ok_or(Stop::Cancelled),
        None => Err(Stop::Cancelled),
    }
}

/// A stand-in picture for dry runs.
fn dry(ctx: &Ctx, what: &str) -> RgbaImage {
    ctx.say(&format!("+ {what}"));
    RgbaImage::new(1, 1)
}

/// KDE Wayland: KWin's ScreenShot2, called from this process (KWin
/// authorizes our binary by its .desktop entry). Region: the whole
/// desktop first, then our selector over that frozen picture.
fn kwin(ctx: &Ctx, mode: &str) -> Result<RgbaImage, Stop> {
    let call = match mode {
        "window" => grab::Call::Window,
        "monitor" => grab::Call::Screen,
        _ => grab::Call::Workspace,
    };
    if ctx.dry() {
        let what = match call {
            grab::Call::Window => "window",
            grab::Call::Screen => "screen",
            _ => "workspace",
        };
        let img = dry(ctx, &format!("kwin-screenshot {what}"));
        if mode == "region" {
            ctx.say("+ select area (layer-shell)");
        }
        return Ok(img);
    }
    let img = grab::image(ctx, &call).map_err(|e| failed(format!("screenshot failed: {e}")))?;
    if mode != "region" {
        return Ok(img);
    }
    let mut wl = wayland(ctx)?;
    let shot = [Shot { img, area: wl.desktop() }];
    pick(select::select(&mut wl, &shot), &shot[0])
}

/// wlroots (Sway, Hyprland, others): screencopy, IPC for the focused
/// window and monitor. A compositor without screencopy gets the portal.
fn screencopy(ctx: &Ctx, mode: &str) -> Result<RgbaImage, Stop> {
    let desk = detect::desktop(ctx);
    if ctx.dry() {
        return Ok(match mode {
            "window" => dry(ctx, "screencopy window (focused, via IPC)"),
            "monitor" => dry(ctx, "screencopy output (focused, via IPC)"),
            "region" => {
                let i = dry(ctx, "screencopy all");
                ctx.say("+ select area (layer-shell)");
                i
            }
            _ => dry(ctx, "screencopy all"),
        });
    }
    let mut wl = wayland(ctx)?;
    if !wl.can_capture() {
        if desk == Desktop::Other {
            return portal(ctx, mode, Some(wl));
        }
        return Err(failed("this compositor offers no screen capture (wlr-screencopy or ext-image-copy-capture)"));
    }
    match mode {
        "monitor" => {
            let i = match ipc::focused_output(ctx) {
                Ok(name) => wl.st.outputs.iter().position(|o| o.name == name).ok_or_else(|| failed(format!("monitor {name} not found")))?,
                Err(_) if wl.st.outputs.len() == 1 => 0,
                Err(e) => {
                    ctx.warn(&format!("can't tell the focused monitor ({e}); taking {}", wl.st.outputs[0].name));
                    0
                }
            };
            wl.capture_output(i).map(|s| s.img).map_err(Stop::Failed)
        }
        "window" => {
            let r = match ipc::active_window(ctx) {
                Ok(r) => r,
                Err(e) if matches!(desk, Desktop::Hyprland | Desktop::Sway) => return Err(failed(e)),
                Err(_) => return region_wl(&mut wl), // no way to ask this compositor: let the user drag it
            };
            let shot = wl.capture_area(r).map_err(Stop::Failed)?;
            shot.crop(r).ok_or_else(|| failed("the window is off screen"))
        }
        "region" => region_wl(&mut wl),
        _ => wl.capture_area(wl.desktop()).map(|s| s.img).map_err(Stop::Failed),
    }
}

/// Every monitor frozen, the selector over them, the picked part at the
/// density of the monitors it covers.
fn region_wl(wl: &mut Wl) -> Result<RgbaImage, Stop> {
    let parts = wl.capture_parts(wl.desktop()).map_err(Stop::Failed)?;
    match select::select(wl, &parts).map_err(|e| failed(format!("area selection: {e}")))? {
        Some(r) => pixels::crop_parts(&parts, r).ok_or(Stop::Cancelled),
        None => Err(Stop::Cancelled),
    }
}

/// GNOME (and Wayland desktops without screencopy): the Screenshot
/// portal. Monitor = the primary monitor; region = our selector over the
/// frozen picture (fullscreen windows, Mutter has no layer-shell);
/// window = the desktop's own interactive screenshot UI.
fn portal(ctx: &Ctx, mode: &str, wl: Option<Wl>) -> Result<RgbaImage, Stop> {
    if ctx.dry() {
        let img = dry(ctx, if mode == "window" { "portal screenshot interactive" } else { "portal screenshot" });
        match mode {
            "region" => ctx.say("+ select area (fullscreen windows)"),
            "monitor" => ctx.say("+ crop to the primary monitor"),
            _ => {}
        }
        return Ok(img);
    }
    let shoot = |interactive| match portal::screenshot(ctx, interactive) {
        Ok(Some(i)) => Ok(i),
        Ok(None) => Err(Stop::Cancelled),
        Err(e) => Err(failed(format!("screenshot failed: {e}"))),
    };
    if mode == "window" {
        return shoot(true);
    }
    let img = shoot(false)?;
    if mode == "all" {
        return Ok(img);
    }
    let mut wl = match wl.map(Ok).unwrap_or_else(|| wayland(ctx)) {
        Ok(wl) => wl,
        Err(_) if mode == "monitor" => return Ok(img), // no layout known: the whole picture
        Err(e) => return Err(e),
    };
    let shot = [Shot { img, area: wl.desktop() }];
    if mode == "monitor" {
        let outs: Vec<Rect> = wl.st.outputs.iter().map(|o| o.logical).collect();
        let primary = portal::gnome_primary(ctx).and_then(|(x, y)| outs.iter().find(|r| (r.x, r.y) == (x, y)).copied());
        return shot[0].crop(primary.unwrap_or(outs[0])).ok_or_else(|| failed("monitor not in the picture"));
    }
    pick(select::select(&mut wl, &shot), &shot[0])
}

/// X11: GetImage on the root window; monitors from RandR.
fn xshot(ctx: &Ctx, mode: &str) -> Result<RgbaImage, Stop> {
    if ctx.dry() {
        let img = dry(ctx, "x11 getimage root");
        match mode {
            "region" => ctx.say("+ select area (x11 window)"),
            "window" => ctx.say("+ crop to the active window"),
            "monitor" => ctx.say("+ crop to the monitor under the pointer"),
            _ => {}
        }
        return Ok(img);
    }
    let x = x11(ctx)?;
    let shot = x.capture().map_err(Stop::Failed)?;
    match mode {
        "monitor" => shot.crop(x.current_monitor()).ok_or_else(|| failed("monitor not in the picture")),
        "window" => {
            let r = x.active_window().map_err(Stop::Failed)?;
            shot.crop(r).ok_or_else(|| failed("the window is off screen"))
        }
        "region" => pick(x11::select(&x, &shot), &shot),
        _ => Ok(shot.img),
    }
}

fn capture(ctx: &Ctx, mode: &str) -> Result<RgbaImage, Stop> {
    match detect::capture(ctx) {
        Capture::Kwin => kwin(ctx, mode),
        Capture::Screencopy => screencopy(ctx, mode),
        Capture::Portal => portal(ctx, mode, None),
        Capture::X11 => xshot(ctx, mode),
        Capture::None => Err(failed(format!(
            "no screen to capture on {}/{} (no Wayland or X11 display); see frame --detect",
            detect::desktop_name(ctx),
            detect::session(ctx)
        ))),
    }
}

/// Take, save, copy, notify. Prints the saved file.
pub fn shot(ctx: &Ctx, cfg: &BTreeMap<String, String>, mode: &str) -> Result<(), Stop> {
    let dir = ctx.expand_dir(&cfg["save_dir"]).join("frame/images");
    std::fs::create_dir_all(&dir).map_err(|e| failed(format!("{}: {e}", dir.display())))?;
    let lock = shot_lock(ctx)?;
    let img = capture(ctx, mode)?;
    drop(lock); // the screen is ours again: the next shortcut may start
    save(ctx, cfg, mode, &dir, &img)
}

/// One capture at a time: a second shortcut while the selector is open
/// would otherwise capture our own dimmed overlay. Held until the
/// picture is taken.
fn shot_lock(ctx: &Ctx) -> Result<crate::ctx::Flock, Stop> {
    let dir = ctx.ensure_rundir().map_err(Stop::Failed)?;
    match crate::ctx::Flock::try_new(&dir.join("shot.lock")) {
        Ok(Some(l)) => Ok(l),
        Ok(None) => Err(Stop::Busy),
        Err(e) => Err(failed(format!("shot.lock: {e}"))),
    }
}

/// Encode IMG into DIR (png, jpg or webp, in this process), copy, notify.
fn save(ctx: &Ctx, cfg: &BTreeMap<String, String>, mode: &str, dir: &Path, img: &RgbaImage) -> Result<(), Stop> {
    let fmt = cfg["image_format"].as_str();
    let dest = new_file(dir, mode, fmt);
    let d = dest.to_string_lossy();
    if ctx.dry() {
        ctx.say(&format!("+ save {fmt} {}", crate::ctx::quote1(&d)));
    } else {
        pixels::save(img, fmt, &dest).map_err(Stop::Failed)?;
    }
    let copied = crate::config::yes(&cfg["clipboard"]) && copy(ctx, &dest);
    crate::notify::saved(ctx, crate::config::yes(&cfg["notify"]), &dest, "Screenshot saved", copied);
    ctx.say(&d);
    Ok(())
}

/// Put FILE on the clipboard as image data. True if it is there.
fn copy(ctx: &Ctx, f: &Path) -> bool {
    let mime = mime_of(f);
    let how = match detect::clipboard(ctx) {
        Clipboard::None => {
            ctx.warn("clipboard: no Wayland or X11 display");
            return false;
        }
        c => c.name(),
    };
    if ctx.dry() {
        ctx.say(&format!("+ frame _clip {} {mime} {how}", crate::ctx::quote1(&f.to_string_lossy())));
        return true;
    }
    match serve_detached(ctx, f, mime, how) {
        Ok(()) => true,
        // A compositor that turns data-control down: Xwayland's clipboard.
        Err(e) if how == "data-control" && ctx.var_os("DISPLAY").is_some() => match serve_detached(ctx, f, mime, "x11") {
            Ok(()) => true,
            Err(x) => {
                ctx.warn(&format!("clipboard: {e}; X11: {x}"));
                false
            }
        },
        Err(e) => {
            ctx.warn(&format!("clipboard: {e}"));
            false
        }
    }
}

/// Start `frame _clip FILE MIME HOW` in its own session and wait (up to
/// 3 s) for it to say it holds the selection.
fn serve_detached(ctx: &Ctx, f: &Path, mime: &str, how: &str) -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    let mut c = ctx.command(&ctx.exe.to_string_lossy());
    c.arg("_clip").arg(f).arg(mime).arg(how).stdout(Stdio::piped()).stderr(Stdio::null());
    // SAFETY: setsid is async-signal-safe.
    unsafe {
        c.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut child = c.spawn().map_err(|e| format!("cannot run {}: {e}", ctx.exe.display()))?;
    let mut out = child.stdout.take().ok_or("no pipe")?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut line = Vec::new();
        let mut b = [0u8; 1];
        while out.read(&mut b).unwrap_or(0) == 1 && b[0] != b'\n' {
            line.push(b[0]);
        }
        let _ = tx.send(String::from_utf8_lossy(&line).into_owned());
    });
    std::thread::spawn(move || child.wait()); // reap it if it ends while we run
    match rx.recv_timeout(Duration::from_secs(3)) {
        Ok(l) if l == "ok" => Ok(()),
        Ok(l) if !l.is_empty() => Err(l.trim_start_matches("error: ").to_string()),
        _ => Err("no answer from the clipboard helper".into()),
    }
}

#[cfg(test)]
mod tests {
    use crate::ctx::testenv::T;

    fn images(t: &T) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(t.dir.join("docs/frame/images"))
            .map(|r| r.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        v.sort();
        v
    }

    #[test]
    fn region_png_kde() {
        let t = T::new("region");
        t.conf(&[]);
        let (code, out, err) = t.frame(&["region"]);
        assert_eq!(code, 0, "{err}");
        let l: Vec<&str> = out.lines().collect();
        assert_eq!(l[..2], ["+ kwin-screenshot workspace", "+ select area (layer-shell)"], "{out}");
        assert!(l[2].starts_with(&format!("+ save png {}/docs/frame/images/", t.dir.display())), "{out}");
        assert!(l[3].starts_with("+ frame _clip ") && l[3].ends_with(" image/png data-control"), "{out}");
        assert!(out.contains("x-kde-urls:file://") && out.contains("Copied to clipboard"), "{out}");
        let last = l.last().unwrap();
        let name = last.rsplit('/').next().unwrap();
        assert!(last.starts_with(&format!("{}/docs/frame/images/", t.dir.display())));
        assert_eq!(name.len(), "2026-10-29_14-03-59_region.png".len(), "{name}");
        assert!(name.ends_with("_region.png"));
        assert!(t.dir.join("docs/frame/images").is_dir() && !t.dir.join("docs/frame/videos").exists());
        // Nothing runs outside the binary: no temp files, no tools.
        assert_eq!(std::fs::read_dir(t.dir.join("tmp")).unwrap().count(), 0);
        assert!(!out.contains("slurp") && !out.contains("ffmpeg"), "{out}");
    }

    #[test]
    fn formats_and_switches() {
        let t = T::new("fmts");
        t.conf(&["image_format = webp"]);
        let (_, out, _) = t.frame(&["window"]);
        assert!(out.contains("+ kwin-screenshot window\n") && out.contains("+ save webp ") && out.contains("image/webp"), "{out}");
        t.conf(&["image_format = jpg", "clipboard = no", "notify = off"]);
        let (_, out, _) = t.frame(&["monitor"]);
        assert!(out.contains("+ kwin-screenshot screen\n") && out.contains("+ save jpg ") && out.trim_end().ends_with("_monitor.jpg"), "{out}");
        assert!(!out.contains("notify") && !out.contains("_clip") && !out.contains("Copied"), "{out}");
        t.conf(&[]);
        let (_, out, _) = t.frame(&["all"]);
        assert!(out.contains("+ kwin-screenshot workspace\n"), "{out}");
        assert!(images(&t).is_empty()); // dry run saves nothing
    }

    #[test]
    fn other_desktops() {
        let mut t = T::new("desktops");
        t.conf(&[]);
        t.set("XDG_SESSION_TYPE", "x11");
        for (m, extra) in [("region", "select area (x11 window)"), ("window", "active window"), ("monitor", "under the pointer"), ("all", "")] {
            let (code, out, err) = t.frame(&[m]);
            assert_eq!(code, 0, "{err}");
            assert!(out.starts_with("+ x11 getimage root\n") && out.contains(extra), "{m}: {out}");
            assert!(out.contains(" image/png x11\n"), "{out}");
        }
        t.set("XDG_SESSION_TYPE", "wayland").set("XDG_CURRENT_DESKTOP", "GNOME");
        let (_, out, _) = t.frame(&["window"]);
        assert!(out.contains("+ portal screenshot interactive\n") && out.contains(" image/png focus\n"), "{out}");
        assert!(out.contains("-h string:image-path:") && out.contains("-A open=Open"), "{out}");
        let (_, out, _) = t.frame(&["region"]);
        assert!(out.contains("+ portal screenshot\n+ select area (fullscreen windows)\n"), "{out}");
        let (_, out, _) = t.frame(&["monitor"]);
        assert!(out.contains("+ crop to the primary monitor\n"), "{out}");
        t.set("XDG_CURRENT_DESKTOP", "Hyprland");
        let (_, out, _) = t.frame(&["monitor"]);
        assert!(out.contains("+ screencopy output (focused, via IPC)\n") && out.contains(" data-control\n"), "{out}");
        let (_, out, _) = t.frame(&["region"]);
        assert!(out.contains("+ screencopy all\n+ select area (layer-shell)\n"), "{out}");
        // No display at all.
        t.unset("XDG_SESSION_TYPE").set("XDG_CURRENT_DESKTOP", "none");
        let (code, _, err) = t.frame(&["all"]);
        assert_eq!(code, 1);
        assert!(err.contains("no screen to capture on none/unknown"), "{err}");
    }

    /// Real mode without a reachable display fails cleanly and saves
    /// nothing (the test env has a temp XDG_RUNTIME_DIR and no D-Bus).
    #[test]
    fn real_mode_without_display() {
        let mut t = T::new("real");
        t.conf(&["clipboard = false", "notify = false"]);
        t.unset("FRAME_DRY_RUN");
        for desk in ["sway", "KDE"] {
            t.set("XDG_CURRENT_DESKTOP", desk);
            let (code, out, err) = t.frame(&["all"]);
            assert_eq!(code, 1, "{desk}: out {out:?} err {err:?}");
            assert!(err.contains("Wayland") || err.contains("D-Bus"), "{desk}: {err}");
        }
        assert!(images(&t).is_empty());
    }

    /// The save step in real mode: a picture is encoded in each format.
    #[test]
    fn real_save_formats() {
        let mut t = T::new("realsave");
        t.unset("FRAME_DRY_RUN");
        let img = image::RgbaImage::from_pixel(3, 2, image::Rgba([10, 20, 30, 255]));
        let dir = t.dir.join("docs/frame/images");
        std::fs::create_dir_all(&dir).unwrap();
        for fmt in ["png", "jpg", "webp"] {
            let mut cfg = crate::config::DEFAULTS.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<std::collections::BTreeMap<_, _>>();
            cfg.insert("image_format".into(), fmt.into());
            cfg.insert("clipboard".into(), "false".into());
            cfg.insert("notify".into(), "false".into());
            let ctx = t.ctx();
            assert!(super::save(&ctx, &cfg, "all", &dir, &img).is_ok());
            let f = std::path::PathBuf::from(ctx.output().0.trim());
            assert!(f.extension().unwrap() == fmt && crate::pixels::load(&f).unwrap().dimensions() == (3, 2), "{fmt}");
        }
        assert_eq!(images(&t).len(), 3);
    }

    /// A shortcut while another shot is under way does nothing, quietly.
    #[test]
    fn second_shot_while_busy() {
        let t = T::new("busy");
        t.conf(&[]);
        let dir = t.ctx().ensure_rundir().unwrap();
        let held = crate::ctx::Flock::try_new(&dir.join("shot.lock")).unwrap().unwrap();
        let (code, out, err) = t.frame(&["region"]);
        assert_eq!((code, out.as_str(), err.as_str()), (0, "", ""));
        drop(held);
        let (code, out, _) = t.frame(&["region"]);
        assert!(code == 0 && out.contains("+ kwin-screenshot workspace"), "{out}");
    }

    #[test]
    fn same_second_names() {
        let t = crate::testutil::tempdir("names");
        let a = super::new_file(&t, "all", "png");
        std::fs::write(&a, "").unwrap();
        let b = super::new_file(&t, "all", "png");
        let (an, bn) = (a.to_string_lossy(), b.to_string_lossy());
        if an.trim_end_matches(".png") == bn.trim_end_matches("-2.png") {
            return; // same second: -2 was added
        }
        assert_ne!(a, b);
    }
}
