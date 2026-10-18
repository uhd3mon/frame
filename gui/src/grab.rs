//! `frame-gui --grab MODE [X Y W H] OUT.png`: a screenshot through KWin's
//! own org.kde.KWin.ScreenShot2 D-Bus API, so KDE needs no Spectacle.
//!
//! KWin only answers callers whose /proc/PID/exe is the Exec of a .desktop
//! file with `X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2`
//! (install.sh writes that into frame.desktop). That is why this lives in
//! the compiled binary and not in bin/frame: authorizing a shell tool
//! (gdbus, python) would let any script take screenshots.
//!
//! KWin writes raw pixels into a pipe we pass; the reply says how to read
//! them (width, height, stride, QImage format). We turn that into RGBA PNG.

use std::collections::HashMap;
use std::io::Read;
use std::os::fd::AsFd;
use std::path::Path;

use zbus::zvariant::{Fd, OwnedValue, Value};

const NOT_AUTHORIZED: &str = "org.kde.KWin.ScreenShot2.Error.NoAuthorized";

pub fn main(args: &[String]) -> i32 {
    let (call, out) = match parse_args(args) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("frame-gui: {e}");
            eprintln!("usage: frame-gui --grab all|monitor|window|area X Y W H OUT.png");
            return 2;
        }
    };
    match grab(&call).and_then(|img| write_png(&img, Path::new(&out))) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("frame-gui: {e}");
            1
        }
    }
}

#[derive(Debug, PartialEq)]
enum Call {
    Workspace,
    Screen,
    Window,
    Area(i32, i32, u32, u32),
}

fn parse_args(args: &[String]) -> Result<(Call, String), String> {
    let (call, rest) = match args.first().map(String::as_str) {
        Some("all") => (Call::Workspace, &args[1..]),
        Some("monitor") => (Call::Screen, &args[1..]),
        Some("window") => (Call::Window, &args[1..]),
        Some("area") if args.len() >= 5 => {
            let n = |i: usize| args[i].parse::<i64>().map_err(|_| format!("bad number: {}", args[i]));
            let (x, y, w, h) = (n(1)?, n(2)?, n(3)?, n(4)?);
            let xy = |v: i64| i32::try_from(v).map_err(|_| format!("out of range: {v}"));
            let wh = |v: i64| u32::try_from(v).ok().filter(|&v| v > 0).ok_or(format!("bad size: {v}"));
            (Call::Area(xy(x)?, xy(y)?, wh(w)?, wh(h)?), &args[5..])
        }
        Some("area") => return Err("area needs X Y W H".into()),
        Some(m) => return Err(format!("unknown grab mode: {m}")),
        None => return Err("missing grab mode".into()),
    };
    match rest {
        [out] if !out.is_empty() => Ok((call, out.clone())),
        _ => Err("expected one output file".into()),
    }
}

/// Pixels as KWin sent them, plus how to read them.
struct Raw {
    width: u32,
    height: u32,
    stride: u32,
    format: u32,
    data: Vec<u8>,
}

fn grab(call: &Call) -> Result<Raw, String> {
    let conn = zbus::blocking::Connection::session().map_err(|e| format!("D-Bus session: {e}"))?;
    let proxy = zbus::blocking::Proxy::new(&conn, "org.kde.KWin", "/org/kde/KWin/ScreenShot2", "org.kde.KWin.ScreenShot2")
        .map_err(|e| format!("KWin ScreenShot2: {e}"))?;
    let mut opts: HashMap<&str, Value> = HashMap::new();
    opts.insert("native-resolution", true.into());
    opts.insert("include-cursor", false.into());
    if *call == Call::Window {
        opts.insert("include-decoration", true.into());
        opts.insert("include-shadow", false.into());
    }

    let (mut rd, wr) = std::io::pipe().map_err(|e| format!("pipe: {e}"))?;
    // KWin writes while (or after) it replies; read on a thread so a big
    // image can't fill the pipe and stall both sides.
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        rd.read_to_end(&mut buf).map(|_| buf)
    });
    let reply = {
        let fd = Fd::from(wr.as_fd());
        let r: zbus::Result<HashMap<String, OwnedValue>> = match *call {
            Call::Workspace => proxy.call("CaptureWorkspace", &(opts, fd)),
            Call::Screen => proxy.call("CaptureActiveScreen", &(opts, fd)),
            Call::Window => proxy.call("CaptureActiveWindow", &(opts, fd)),
            Call::Area(x, y, w, h) => proxy.call("CaptureArea", &(x, y, w, h, opts, fd)),
        };
        drop(wr); // KWin holds its own copy; ours must close for EOF
        r
    };
    let data = reader.join().map_err(|_| "pipe reader panicked".to_string())?;
    let reply = reply.map_err(describe)?;
    let data = data.map_err(|e| format!("reading KWin's pipe: {e}"))?;

    let get = |k: &str| -> Result<u32, String> {
        let v = reply.get(k).ok_or(format!("KWin reply has no {k}"))?;
        u32::try_from(v).map_err(|e| format!("KWin reply {k}: {e}"))
    };
    if let Some(t) = reply.get("type") {
        let t: &str = t.downcast_ref().map_err(|e| format!("KWin reply type: {e}"))?;
        if t != "raw" {
            return Err(format!("KWin sent image type '{t}', expected raw"));
        }
    }
    Ok(Raw { width: get("width")?, height: get("height")?, stride: get("stride")?, format: get("format")?, data })
}

fn describe(e: zbus::Error) -> String {
    match &e {
        zbus::Error::MethodError(name, _, _) if name.as_str() == NOT_AUTHORIZED => {
            "not authorized: is frame.desktop installed with an absolute Exec and X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2?".into()
        }
        zbus::Error::MethodError(name, msg, _) if name.as_str() == "org.freedesktop.DBus.Error.ServiceUnknown" => {
            format!("KWin is not running ({})", msg.as_deref().unwrap_or("no org.kde.KWin"))
        }
        zbus::Error::MethodError(name, msg, _) => match msg {
            Some(m) => format!("KWin screenshot failed: {m} ({name})"),
            None => format!("KWin screenshot failed: {name}"),
        },
        _ => format!("KWin screenshot failed: {e}"),
    }
}

/// QImage formats we read (QImage::Format values).
const RGB32: u32 = 4; // 0xffRRGGBB, native-endian u32
const ARGB32: u32 = 5; // 0xAARRGGBB
const ARGB32_PREMUL: u32 = 6;
const RGBX8888: u32 = 16; // bytes R G B x
const RGBA8888: u32 = 17;
const RGBA8888_PREMUL: u32 = 18;

/// Raw rows -> tightly packed RGBA8 and whether every pixel is opaque.
fn to_rgba(raw: &Raw) -> Result<(Vec<u8>, bool), String> {
    let (w, h, stride) = (raw.width as usize, raw.height as usize, raw.stride as usize);
    if w == 0 || h == 0 {
        return Err("KWin sent an empty image".into());
    }
    if stride < w * 4 {
        return Err(format!("bad stride {stride} for width {w}"));
    }
    let need = stride * (h - 1) + w * 4;
    if raw.data.len() < need {
        return Err(format!("KWin sent {} bytes, expected {need} ({w}x{h}, stride {stride})", raw.data.len()));
    }
    let px: fn([u8; 4]) -> [u8; 4] = match raw.format {
        RGB32 => |b| argb(u32::from_ne_bytes(b) | 0xff00_0000),
        ARGB32 => |b| argb(u32::from_ne_bytes(b)),
        ARGB32_PREMUL => |b| unpremultiply(argb(u32::from_ne_bytes(b))),
        RGBX8888 => |[r, g, b, _]| [r, g, b, 255],
        RGBA8888 => |b| b,
        RGBA8888_PREMUL => unpremultiply,
        f => return Err(format!("KWin sent unsupported image format {f}")),
    };
    let mut out = Vec::with_capacity(w * h * 4);
    let mut opaque = true;
    for row in raw.data.chunks(stride).take(h) {
        for p in row[..w * 4].as_chunks::<4>().0 {
            let c = px(*p);
            opaque &= c[3] == 255;
            out.extend_from_slice(&c);
        }
    }
    Ok((out, opaque))
}

fn argb(v: u32) -> [u8; 4] {
    [(v >> 16) as u8, (v >> 8) as u8, v as u8, (v >> 24) as u8]
}

fn unpremultiply([r, g, b, a]: [u8; 4]) -> [u8; 4] {
    match a {
        0 => [0, 0, 0, 0],
        255 => [r, g, b, a],
        _ => {
            let f = |c: u8| ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8;
            [f(r), f(g), f(b), a]
        }
    }
}

/// Write OUT via a temp file beside it, so a failed grab leaves nothing.
/// Opaque shots (the usual case) are stored as RGB: a quarter smaller.
fn write_png(raw: &Raw, out: &Path) -> Result<(), String> {
    let (rgba, opaque) = to_rgba(raw)?;
    let tmp = out.with_file_name(format!(".{}.part", out.file_name().and_then(|n| n.to_str()).unwrap_or("grab")));
    let r = (|| -> Result<(), String> {
        let f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        let mut enc = png::Encoder::new(std::io::BufWriter::new(f), raw.width, raw.height);
        enc.set_depth(png::BitDepth::Eight);
        let data = if opaque {
            enc.set_color(png::ColorType::Rgb);
            rgba.as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect()
        } else {
            enc.set_color(png::ColorType::Rgba);
            rgba
        };
        let mut w = enc.write_header().map_err(|e| e.to_string())?;
        w.write_image_data(&data).map_err(|e| e.to_string())?;
        w.finish().map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, out).map_err(|e| e.to_string())
    })();
    if r.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    r.map_err(|e| format!("{}: {e}", out.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn le(v: u32) -> [u8; 4] {
        v.to_ne_bytes()
    }

    #[test]
    fn args() {
        assert_eq!(parse_args(&s(&["all", "/o.png"])), Ok((Call::Workspace, "/o.png".into())));
        assert_eq!(parse_args(&s(&["monitor", "o"])).unwrap().0, Call::Screen);
        assert_eq!(parse_args(&s(&["window", "o"])).unwrap().0, Call::Window);
        assert_eq!(parse_args(&s(&["area", "-10", "20", "300", "40", "o"])).unwrap().0, Call::Area(-10, 20, 300, 40));
        for bad in [&[][..], &["all"], &["all", "a", "b"], &["area", "1", "2", "3", "o"], &["area", "1", "2", "0", "4", "o"],
            &["area", "1", "2", "-3", "4", "o"], &["area", "x", "2", "3", "4", "o"], &["region", "o"], &["all", ""]]
        {
            assert!(parse_args(&s(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn premultiplied_with_stride_padding() {
        // 2x2, stride 12: 4 bytes of junk after each row.
        let mut data = Vec::new();
        data.extend(le(0xff10_2030)); // opaque
        data.extend(le(0x8040_2000)); // half alpha, premultiplied (0x80,0x40,0x00 -> 0xff,0x80,0x00)
        data.extend([0xee; 4]);
        data.extend(le(0x0000_0000)); // transparent
        data.extend(le(0x0101_0101)); // alpha 1: value 1 -> 255
        data.extend([0xee; 4]);
        let raw = Raw { width: 2, height: 2, stride: 12, format: ARGB32_PREMUL, data };
        let (px, opaque) = to_rgba(&raw).unwrap();
        assert!(!opaque);
        assert_eq!(px, [0x10, 0x20, 0x30, 0xff, 0x80, 0x40, 0x00, 0x80, 0, 0, 0, 0, 255, 255, 255, 1]);
    }

    #[test]
    fn last_row_may_be_short() {
        // stride * height is not required: the padding of the last row may be missing.
        let mut data = Vec::new();
        data.extend(le(0x00ab_cdef));
        data.extend([0; 4]);
        data.extend(le(0x0012_3456));
        let raw = Raw { width: 1, height: 2, stride: 8, format: RGB32, data };
        let (px, opaque) = to_rgba(&raw).unwrap();
        assert!(opaque, "RGB32 ignores the x byte");
        assert_eq!(px, [0xab, 0xcd, 0xef, 255, 0x12, 0x34, 0x56, 255]);
    }

    #[test]
    fn other_formats_and_errors() {
        let one = |format, b: [u8; 4]| to_rgba(&Raw { width: 1, height: 1, stride: 4, format, data: b.to_vec() });
        assert_eq!(one(ARGB32, le(0x8040_2000)).unwrap().0, [0x40, 0x20, 0x00, 0x80]);
        assert_eq!(one(RGBX8888, [1, 2, 3, 0]).unwrap(), (vec![1, 2, 3, 255], true));
        assert_eq!(one(RGBA8888, [1, 2, 3, 4]).unwrap().0, [1, 2, 3, 4]);
        assert_eq!(one(RGBA8888_PREMUL, [0x40, 0x20, 0, 0x80]).unwrap().0, [0x80, 0x40, 0, 0x80]);
        assert!(one(99, [0; 4]).unwrap_err().contains("unsupported image format 99"));
        let short = Raw { width: 2, height: 2, stride: 8, format: ARGB32, data: vec![0; 12] };
        assert!(to_rgba(&short).unwrap_err().contains("expected 16"));
        let narrow = Raw { width: 2, height: 1, stride: 4, format: ARGB32, data: vec![0; 8] };
        assert!(to_rgba(&narrow).unwrap_err().contains("bad stride"));
    }

    #[test]
    fn png_round_trip() {
        let d = crate::testutil::tempdir("grab");
        let out = d.join("shot.png");
        for (format, px, want) in [
            (RGB32, le(0x0011_2233), png::ColorType::Rgb),
            (ARGB32_PREMUL, le(0x8040_2000), png::ColorType::Rgba),
        ] {
            let mut data = Vec::new();
            for _ in 0..6 {
                data.extend(px);
            }
            let raw = Raw { width: 3, height: 2, stride: 12, format, data };
            write_png(&raw, &out).unwrap();
            let mut r = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&out).unwrap())).read_info().unwrap();
            let mut buf = vec![0; r.output_buffer_size().unwrap()];
            let info = r.next_frame(&mut buf).unwrap();
            assert_eq!((info.width, info.height, info.color_type), (3, 2, want));
            let first = &buf[..if want == png::ColorType::Rgb { 3 } else { 4 }];
            let expect = &to_rgba(&raw).unwrap().0[..first.len()];
            assert_eq!(first, expect);
        }
        let names: Vec<_> = std::fs::read_dir(&*d).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, ["shot.png"], "no .part left behind");
        let bad = Raw { width: 1, height: 1, stride: 4, format: 99, data: vec![0; 4] };
        assert!(write_png(&bad, &d.join("x.png")).is_err());
        assert!(!d.join("x.png").exists());
    }
}
