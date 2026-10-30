//! Hyprland's and Sway's IPC sockets, spoken directly (no hyprctl or
//! swaymsg): the focused window's rectangle and the focused monitor.

use crate::ctx::Ctx;
use crate::detect::{self, Desktop};
use crate::pixels::Rect;
use serde_json::Value;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

fn connect(p: PathBuf) -> Result<UnixStream, String> {
    let s = UnixStream::connect(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    let t = Some(Duration::from_secs(3));
    let _ = s.set_read_timeout(t);
    let _ = s.set_write_timeout(t);
    Ok(s)
}

/// Hyprland: "j/<cmd>" on .socket.sock; the answer is JSON until EOF.
fn hypr(ctx: &Ctx, cmd: &str) -> Result<Value, String> {
    let sig = ctx.var("HYPRLAND_INSTANCE_SIGNATURE");
    if sig.is_empty() {
        return Err("HYPRLAND_INSTANCE_SIGNATURE is not set".into());
    }
    let mut dirs = Vec::new();
    if let Some(r) = ctx.var_os("XDG_RUNTIME_DIR") {
        dirs.push(PathBuf::from(r).join("hypr"));
    }
    dirs.push(PathBuf::from("/tmp/hypr")); // Hyprland before 0.40
    let p = dirs.into_iter().map(|d| d.join(&sig).join(".socket.sock")).find(|p| p.exists()).ok_or("Hyprland's socket was not found")?;
    let mut s = connect(p)?;
    s.write_all(format!("j/{cmd}").as_bytes()).map_err(|e| format!("Hyprland IPC: {e}"))?;
    let mut out = Vec::new();
    s.read_to_end(&mut out).map_err(|e| format!("Hyprland IPC: {e}"))?;
    serde_json::from_slice(&out).map_err(|e| format!("Hyprland IPC: {e}"))
}

/// i3/Sway IPC: "i3-ipc", u32 length, u32 type (native endian), payload.
pub fn sway_msg(kind: u32, payload: &[u8]) -> Vec<u8> {
    let mut m = b"i3-ipc".to_vec();
    m.extend((payload.len() as u32).to_ne_bytes());
    m.extend(kind.to_ne_bytes());
    m.extend(payload);
    m
}

fn sway(ctx: &Ctx, kind: u32) -> Result<Value, String> {
    let p = ctx.var_os("SWAYSOCK").ok_or("SWAYSOCK is not set")?;
    let mut s = connect(PathBuf::from(p))?;
    let e = |e: std::io::Error| format!("Sway IPC: {e}");
    s.write_all(&sway_msg(kind, b"")).map_err(e)?;
    let mut head = [0u8; 14];
    s.read_exact(&mut head).map_err(e)?;
    if &head[..6] != b"i3-ipc" {
        return Err("Sway IPC: bad reply".into());
    }
    let len = u32::from_ne_bytes(head[6..10].try_into().expect("4 bytes")) as usize;
    let mut body = vec![0u8; len];
    s.read_exact(&mut body).map_err(e)?;
    serde_json::from_slice(&body).map_err(|e| format!("Sway IPC: {e}"))
}

const GET_OUTPUTS: u32 = 3;
const GET_TREE: u32 = 4;

/// The focused window, in desktop coordinates.
pub fn active_window(ctx: &Ctx) -> Result<Rect, String> {
    match detect::desktop(ctx) {
        Desktop::Hyprland => hypr_window(&hypr(ctx, "activewindow")?),
        Desktop::Sway => sway_window(&sway(ctx, GET_TREE)?),
        _ => Err("no window information on this desktop".into()),
    }
}

/// The focused monitor's name.
pub fn focused_output(ctx: &Ctx) -> Result<String, String> {
    let v = match detect::desktop(ctx) {
        Desktop::Hyprland => hypr(ctx, "monitors")?,
        Desktop::Sway => sway(ctx, GET_OUTPUTS)?,
        _ => return Err("no monitor information on this desktop".into()),
    };
    focused_name(&v).ok_or_else(|| "no focused monitor".into())
}

fn hypr_window(w: &Value) -> Result<Rect, String> {
    let n = |v: &Value, i: usize| v.get(i).and_then(Value::as_i64).map(|x| x as i32);
    (|| Some(Rect::new(n(&w["at"], 0)?, n(&w["at"], 1)?, n(&w["size"], 0)?, n(&w["size"], 1)?)))()
        .filter(|r| !r.is_empty())
        .ok_or_else(|| "no focused window".into())
}

fn sway_window(tree: &Value) -> Result<Rect, String> {
    let n = find_focused(tree).ok_or("no focused window")?;
    let r = &n["rect"];
    let g = |k: &str| r.get(k).and_then(Value::as_i64).map(|x| x as i32);
    let rect = (|| Some(Rect::new(g("x")?, g("y")?, g("width")?, g("height")?)))().ok_or("no window rectangle")?;
    if n["type"] == "workspace" || n["type"] == "output" || rect.is_empty() {
        return Err("no focused window".into());
    }
    Ok(rect)
}

/// The first object anywhere in V with "focused": true.
fn find_focused(v: &Value) -> Option<&Value> {
    match v {
        Value::Object(m) => {
            if m.get("focused").and_then(Value::as_bool) == Some(true) {
                return Some(v);
            }
            m.values().find_map(find_focused)
        }
        Value::Array(a) => a.iter().find_map(find_focused),
        _ => None,
    }
}

fn focused_name(v: &Value) -> Option<String> {
    v.as_array()?.iter().find(|o| o["focused"].as_bool() == Some(true))?["name"].as_str().map(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses() {
        assert_eq!(hypr_window(&json!({"at": [10, 20], "size": [300, 200]})), Ok(Rect::new(10, 20, 300, 200)));
        assert!(hypr_window(&json!({})).is_err());
        let tree = json!({"type": "root", "nodes": [{"type": "workspace", "nodes": [
            {"type": "con", "focused": true, "rect": {"x": 5, "y": 30, "width": 100, "height": 70}}]}]});
        assert_eq!(sway_window(&tree), Ok(Rect::new(5, 30, 100, 70)));
        let empty_ws = json!({"nodes": [{"type": "workspace", "focused": true, "rect": {"x": 0, "y": 0, "width": 9, "height": 9}}]});
        assert!(sway_window(&empty_ws).is_err());
        assert_eq!(focused_name(&json!([{"name": "A"}, {"name": "B", "focused": true}])), Some("B".into()));
        assert_eq!(&sway_msg(4, b"")[..6], b"i3-ipc");
        assert_eq!(sway_msg(4, b"x").len(), 15);
    }

    /// A fake Sway socket answering GET_TREE.
    #[test]
    fn sway_socket() {
        let d = crate::testutil::tempdir("swayipc");
        let sock = d.join("s.sock");
        let l = std::os::unix::net::UnixListener::bind(&sock).unwrap();
        let srv = std::thread::spawn(move || {
            let (mut c, _) = l.accept().unwrap();
            let mut head = [0u8; 14];
            c.read_exact(&mut head).unwrap();
            assert_eq!(u32::from_ne_bytes(head[10..14].try_into().unwrap()), GET_TREE);
            let body = br#"{"nodes":[{"type":"con","focused":true,"rect":{"x":1,"y":2,"width":3,"height":4}}]}"#;
            c.write_all(&sway_msg(GET_TREE, body)).unwrap();
        });
        let mut t = crate::ctx::testenv::T::new("swayipc");
        t.set("XDG_CURRENT_DESKTOP", "sway").set("SWAYSOCK", sock.to_str().unwrap());
        assert_eq!(active_window(&t.ctx()), Ok(Rect::new(1, 2, 3, 4)));
        srv.join().unwrap();
    }
}
