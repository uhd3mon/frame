//! Notifications through org.freedesktop.Notifications (D-Bus), with a
//! preview of the screenshot and Open / Show in folder buttons.

use crate::ctx::{file_uri, quote, Ctx};
use crate::detect::{desktop, Desktop};
use std::collections::HashMap;
use std::path::Path;
use zbus::zvariant::Value;

const DEST: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";

struct Note<'a> {
    icon: &'a str,
    summary: &'a str,
    body: String,
    hints: Vec<(&'static str, Value<'a>)>,
    actions: bool,
}

/// A "+ notify ..." line for dry-run, in notify-send's syntax.
fn dry_line(n: &Note) -> String {
    let mut a: Vec<String> = vec!["notify".into(), "-a".into(), "frame".into()];
    if !n.icon.is_empty() {
        a.extend(["-i".into(), n.icon.into()]);
    }
    for (k, v) in &n.hints {
        let v = match v {
            Value::Str(s) => format!("string:{k}:{s}"),
            Value::U8(b) => format!("byte:{k}:{b}"),
            other => format!("{k}:{other}"),
        };
        a.extend(["-h".into(), v]);
    }
    if n.actions {
        a.extend(["-A".into(), "open=Open".into(), "-A".into(), "folder=Show in folder".into()]);
    }
    a.extend([n.summary.into(), n.body.clone()]);
    format!("+ {}", quote(&a.iter().map(String::as_str).collect::<Vec<_>>()))
}

/// Send N on CONN; returns the notification id.
fn send(conn: &zbus::blocking::Connection, n: &Note) -> zbus::Result<u32> {
    let proxy = zbus::blocking::Proxy::new(conn, DEST, PATH, DEST)?;
    let mut actions: Vec<&str> = Vec::new();
    if n.actions {
        let caps: Vec<String> = proxy.call("GetCapabilities", &()).unwrap_or_default();
        if caps.iter().any(|c| c == "actions") {
            actions = vec!["open", "Open", "folder", "Show in folder"];
        }
    }
    let hints: HashMap<&str, &Value> = n.hints.iter().map(|(k, v)| (*k, v)).collect();
    proxy.call("Notify", &("frame", 0u32, n.icon, n.summary, n.body.as_str(), actions, hints, -1i32))
}

/// A failure the user should see (shortcuts have no terminal).
pub fn error(ctx: &Ctx, enabled: bool, msg: &str) {
    if !enabled {
        return;
    }
    let n = Note { icon: "dialog-error", summary: "frame", body: markup(msg), hints: vec![("urgency", Value::U8(2))], actions: false };
    if ctx.dry() {
        ctx.say(&dry_line(&n));
    } else if let Ok(conn) = ctx.session_bus() {
        let _ = send(&conn, &n);
    }
}

fn markup(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// The saved-screenshot toast. It waits for a button press, so the real
/// one runs as `frame _toast FILE TITLE COPIED`, detached from the shortcut.
pub fn saved(ctx: &Ctx, enabled: bool, file: &Path, title: &str, copied: bool) {
    if !enabled {
        return;
    }
    if ctx.dry() {
        toast(ctx, file, title, copied);
        return;
    }
    let mut c = ctx.command(&ctx.exe.to_string_lossy());
    c.arg("_toast").arg(file).arg(title).arg(if copied { "1" } else { "0" });
    if let Err(e) = crate::ctx::spawn_detached(&mut c) {
        ctx.warn(&format!("cannot show the notification: {e}"));
    }
}

/// Show the toast and act on its buttons (at most 30 minutes).
/// KDE gets x-kde-urls (a thumbnail you can drag out, like Spectacle);
/// other servers get the image as image-path.
pub fn toast(ctx: &Ctx, file: &Path, title: &str, copied: bool) {
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut body = markup(&name);
    if copied {
        body.push_str("\nCopied to clipboard");
    }
    let uri = file_uri(file);
    let path = file.to_string_lossy().into_owned();
    let (icon, hints) = if desktop(ctx) == Desktop::Kde {
        ("camera-photo", vec![("x-kde-urls", Value::from(uri.as_str()))])
    } else {
        ("", vec![("image-path", Value::from(path.as_str()))])
    };
    let n = Note { icon, summary: title, body, hints, actions: true };
    if ctx.dry() {
        ctx.say(&dry_line(&n));
        return;
    }
    if let Err(e) = wait_and_act(ctx, &n, file) {
        ctx.warn(&format!("notification: {e}"));
    }
}

fn wait_and_act(ctx: &Ctx, n: &Note, file: &Path) -> Result<(), String> {
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(30 * 60));
        std::process::exit(0);
    });
    // Listen before sending, so a quick click isn't missed.
    let conn = ctx.session_bus()?;
    listen(ctx, &conn, n, file).map_err(|e| e.to_string())
}

fn listen(ctx: &Ctx, conn: &zbus::blocking::Connection, n: &Note, file: &Path) -> zbus::Result<()> {
    let rule = zbus::MatchRule::builder().msg_type(zbus::message::Type::Signal).interface(DEST)?.path(PATH)?.build();
    let signals = zbus::blocking::MessageIterator::for_match_rule(rule, conn, None)?;
    let id = send(conn, n)?;
    for m in signals {
        let m = m?;
        let member = m.header().member().map(|s| s.to_string()).unwrap_or_default();
        match member.as_str() {
            "ActionInvoked" => {
                let (nid, key): (u32, String) = m.body().deserialize()?;
                if nid != id {
                    continue;
                }
                match key.as_str() {
                    "open" => open_file(ctx, file),
                    "folder" => show_in_folder(ctx, file),
                    _ => continue,
                }
                return Ok(());
            }
            "NotificationClosed" => {
                let (nid, _why): (u32, u32) = m.body().deserialize()?;
                if nid == id {
                    return Ok(());
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// The file manager with FILE selected, else its folder.
pub fn show_in_folder(ctx: &Ctx, file: &Path) {
    let shown = ctx.session_bus().and_then(|conn| {
        let p = zbus::blocking::Proxy::new(&conn, "org.freedesktop.FileManager1", "/org/freedesktop/FileManager1", "org.freedesktop.FileManager1")
            .map_err(|e| e.to_string())?;
        p.call::<_, _, ()>("ShowItems", &(vec![file_uri(file)], "")).map_err(|e| e.to_string())
    });
    if shown.is_err() && !crate::portal::open(ctx, file, true) {
        if let Some(dir) = file.parent() {
            let _ = ctx.command("xdg-open").arg(dir).status(); // last resort, if installed
        }
    }
}

/// FILE in the user's image viewer: the desktop portal (OpenURI), else
/// xdg-open as a last resort if it happens to be installed.
fn open_file(ctx: &Ctx, file: &Path) {
    if !crate::portal::open(ctx, file, false) {
        let _ = ctx.command("xdg-open").arg(file).status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::testenv::T;

    #[test]
    fn dry_toasts() {
        let mut t = T::new("toast");
        let f = t.dir.join("docs/a <b>.png");
        let ctx = t.ctx();
        toast(&ctx, &f, "Screenshot saved", true);
        let out = ctx.output().0;
        let uri = file_uri(&f);
        assert_eq!(
            out,
            format!(
                "+ notify -a frame -i camera-photo -h string:x-kde-urls:{uri} -A open=Open -A 'folder=Show in folder' 'Screenshot saved' 'a &lt;b&gt;.png\nCopied to clipboard'\n"
            )
        );
        t.set("XDG_CURRENT_DESKTOP", "GNOME");
        let ctx = t.ctx();
        saved(&ctx, true, &f, "Screenshot saved", false);
        assert!(ctx.output().0.starts_with(&format!("+ notify -a frame -h 'string:image-path:{}'", f.display())));
        let ctx = t.ctx();
        saved(&ctx, false, &f, "x", false);
        error(&ctx, false, "x");
        assert_eq!(ctx.output().0, "");
        error(&ctx, true, "a<b");
        assert_eq!(ctx.output().0, "+ notify -a frame -i dialog-error -h byte:urgency:2 frame 'a&lt;b'\n");
    }
}
