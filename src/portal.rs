//! xdg-desktop-portal over D-Bus (no gnome-screenshot, no xdg-open):
//! Screenshot for GNOME (Mutter lets no other client copy the screen),
//! and OpenFile / OpenDirectory for the toast's buttons.
//!
//! The first non-interactive screenshot may make GNOME ask once whether
//! Frame may take screenshots (the portal's "screenshot" permission; it
//! can be changed later in Settings -> Apps). Interactive mode (window)
//! opens GNOME's own screenshot UI.

use crate::ctx::Ctx;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

const DEST: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";

/// Token for request object paths: letters, digits and _ only.
fn token() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    format!("frame{}_{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed))
}

/// /org/freedesktop/portal/desktop/request/SENDER/TOKEN, SENDER = our
/// unique name without ':' and with '.' as '_'.
fn request_path(unique: &str, token: &str) -> String {
    format!("{PATH}/request/{}/{token}", unique.trim_start_matches(':').replace('.', "_"))
}

type Call<'a> = Box<dyn FnOnce(&zbus::blocking::Proxy, &str) -> zbus::Result<OwnedObjectPath> + 'a>;

/// Make CALL on IFACE (it gets a fresh handle_token) and wait for the
/// request's Response. Ok(None) = the user cancelled.
fn request(ctx: &Ctx, iface: &str, call: Call, wait: Duration) -> Result<Option<HashMap<String, OwnedValue>>, String> {
    let conn = ctx.session_bus()?;
    let e = |e: zbus::Error| format!("desktop portal: {e}");
    let tok = token();
    let unique = conn.unique_name().map(|n| n.to_string()).ok_or("no D-Bus name")?;
    let expect = request_path(&unique, &tok);
    // Listen before calling, so a quick answer isn't missed.
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("org.freedesktop.portal.Request")
        .map_err(e)?
        .member("Response")
        .map_err(e)?
        .build();
    let signals = zbus::blocking::MessageIterator::for_match_rule(rule, &conn, Some(16)).map_err(e)?;
    let proxy = zbus::blocking::Proxy::new(&conn, DEST, PATH, iface).map_err(e)?;
    let handle = call(&proxy, &tok).map_err(|err| match err {
        zbus::Error::MethodError(n, _, _) if n.as_str() == "org.freedesktop.DBus.Error.ServiceUnknown" => {
            "no desktop portal is running (xdg-desktop-portal)".to_string()
        }
        other => e(other),
    })?;
    let paths = [expect, handle.as_str().to_string()];
    let (tx, rx) = std::sync::mpsc::channel();
    let started = SystemTime::now();
    std::thread::spawn(move || {
        for m in signals {
            let Ok(m) = m else { continue };
            let p = m.header().path().map(|p| p.to_string()).unwrap_or_default();
            if paths.contains(&p) {
                let r = m.body().deserialize::<(u32, HashMap<String, OwnedValue>)>().map_err(|e| e.to_string());
                if let Err(std::sync::mpsc::SendError(Ok((0, late)))) = tx.send(r) {
                    // We gave up already: don't leave its picture behind.
                    if let Some(f) = late.get("uri").and_then(|v| String::try_from(v.clone()).ok()).and_then(|u| uri_path(&u)) {
                        remove_fresh(&f, started);
                    }
                }
                return;
            }
        }
    });
    let r = rx.recv_timeout(wait);
    if r.is_err() {
        // Withdraw the request so the portal doesn't save a picture
        // nobody takes (a late answer is cleaned up above while we run).
        if let Ok(req) = zbus::blocking::Proxy::new(&conn, DEST, handle.as_str(), "org.freedesktop.portal.Request") {
            let _ = req.call_method("Close", &());
        }
    }
    match r {
        Ok(Ok((0, results))) => Ok(Some(results)),
        Ok(Ok((1, _))) => Ok(None),
        Ok(Ok((code, _))) => Err(format!("the desktop portal refused ({code}); check Settings -> Apps -> Frame -> Screenshots")),
        Ok(Err(err)) => Err(format!("desktop portal: {err}")),
        Err(_) => Err("the desktop portal did not answer".into()),
    }
}

/// A screenshot through the portal. INTERACTIVE = the desktop's own UI
/// (the user picks window/area). Ok(None) = cancelled.
pub fn screenshot(ctx: &Ctx, interactive: bool) -> Result<Option<image::RgbaImage>, String> {
    let started = SystemTime::now();
    let wait = Duration::from_secs(if interactive { 600 } else { 120 });
    let res = request(
        ctx,
        "org.freedesktop.portal.Screenshot",
        Box::new(move |p, tok| {
            let mut o: HashMap<&str, Value> = HashMap::new();
            o.insert("handle_token", tok.into());
            o.insert("interactive", interactive.into());
            o.insert("modal", false.into());
            p.call("Screenshot", &("", o))
        }),
        wait,
    )?;
    let Some(res) = res else { return Ok(None) };
    let uri: String = res.get("uri").and_then(|v| String::try_from(v.clone()).ok()).ok_or("the portal sent no picture")?;
    let path = uri_path(&uri).ok_or_else(|| format!("the portal sent an unexpected address: {uri}"))?;
    let img = crate::pixels::load(&path);
    // The portal wrote that file only for us (often into ~/Pictures);
    // remove it so the picture isn't saved twice.
    remove_fresh(&path, started);
    img.map(Some)
}

/// Remove F if it was written since STARTED (a portal's screenshot file,
/// never an older file of the user's).
fn remove_fresh(f: &Path, started: SystemTime) {
    let fresh = std::fs::metadata(f).and_then(|m| m.modified()).is_ok_and(|t| t + Duration::from_secs(5) >= started);
    if fresh {
        let _ = std::fs::remove_file(f);
    }
}

/// file:///a%20b -> /a b
pub fn uri_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    if !rest.starts_with('/') {
        return None;
    }
    let b = rest.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let h = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok())?;
            out.push(h);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    use std::os::unix::ffi::OsStringExt;
    Some(PathBuf::from(std::ffi::OsString::from_vec(out)))
}

/// GNOME: the primary monitor's top-left corner (logical), from Mutter.
pub fn gnome_primary(ctx: &Ctx) -> Option<(i32, i32)> {
    type Ids = (String, String, String, String);
    type Mode = (String, i32, i32, f64, f64, Vec<f64>, HashMap<String, OwnedValue>);
    type Monitor = (Ids, Vec<Mode>, HashMap<String, OwnedValue>);
    type Logical = (i32, i32, f64, u32, bool, Vec<Ids>, HashMap<String, OwnedValue>);
    let conn = ctx.session_bus().ok()?;
    let p = zbus::blocking::Proxy::new(&conn, "org.gnome.Mutter.DisplayConfig", "/org/gnome/Mutter/DisplayConfig", "org.gnome.Mutter.DisplayConfig").ok()?;
    let (_, _, logical, _): (u32, Vec<Monitor>, Vec<Logical>, HashMap<String, OwnedValue>) = p.call("GetCurrentState", &()).ok()?;
    logical.iter().find(|l| l.4).map(|l| (l.0, l.1))
}

/// Open FILE (OpenFile) or the folder holding it (OpenDirectory) with
/// the user's default app. False if the portal isn't there.
pub fn open(ctx: &Ctx, file: &Path, folder: bool) -> bool {
    let Ok(f) = std::fs::File::open(file) else { return false };
    let Ok(conn) = ctx.session_bus() else { return false };
    let Ok(p) = zbus::blocking::Proxy::new(&conn, DEST, PATH, "org.freedesktop.portal.OpenURI") else { return false };
    let o: HashMap<&str, Value> = HashMap::new();
    let fd = zbus::zvariant::Fd::from(std::os::fd::AsFd::as_fd(&f));
    let method = if folder { "OpenDirectory" } else { "OpenFile" };
    p.call::<_, _, OwnedObjectPath>(method, &("", fd, o)).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths() {
        assert_eq!(request_path(":1.234", "frame9_0"), "/org/freedesktop/portal/desktop/request/1_234/frame9_0");
        assert_eq!(uri_path("file:///home/u/Pictures/Screenshot%20from%20x.png"), Some(PathBuf::from("/home/u/Pictures/Screenshot from x.png")));
        assert_eq!(uri_path("file://localhost/tmp/a.png"), Some(PathBuf::from("/tmp/a.png")));
        assert_eq!(uri_path("file:///%e2%9c%93"), Some(PathBuf::from("/✓")));
        assert_eq!(uri_path("https://x/a.png"), None);
        assert_eq!(uri_path("file:///bad%zz"), None);
        assert_eq!(uri_path("file:///end%2"), Some(PathBuf::from("/end%2")));
        assert!(token().chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
    }
}
