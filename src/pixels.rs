//! Pixels in memory: raw capture buffers -> RGBA, output transforms,
//! stitching monitors into one picture, cropping by desktop (logical)
//! coordinates, and saving as PNG, JPG or WebP. No ffmpeg.

use image::{ImageEncoder, RgbaImage};
use std::path::Path;

/// A rectangle in desktop (logical) coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }

    pub fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }

    pub fn intersect(&self, o: &Rect) -> Option<Rect> {
        let (x, y) = (self.x.max(o.x), self.y.max(o.y));
        let r = Rect::new(x, y, self.right().min(o.right()) - x, self.bottom().min(o.bottom()) - y);
        (!r.is_empty()).then_some(r)
    }

    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && py >= self.y && px < self.right() && py < self.bottom()
    }

    /// The smallest rectangle holding all of RS.
    pub fn bounds(rs: impl IntoIterator<Item = Rect>) -> Option<Rect> {
        rs.into_iter().fold(None, |acc: Option<Rect>, r| {
            Some(match acc {
                None => r,
                Some(a) => {
                    let (x, y) = (a.x.min(r.x), a.y.min(r.y));
                    Rect::new(x, y, a.right().max(r.right()) - x, a.bottom().max(r.bottom()) - y)
                }
            })
        })
    }
}

/// A picture of the desktop area AREA (logical); its pixels may be denser
/// (HiDPI): `scale` = pixels per logical unit.
pub struct Shot {
    pub img: RgbaImage,
    pub area: Rect,
}

impl Shot {
    pub fn scale(&self) -> (f64, f64) {
        (self.img.width() as f64 / self.area.w as f64, self.img.height() as f64 / self.area.h as f64)
    }

    /// The part of the picture under R (logical), clipped to the picture.
    pub fn crop(&self, r: Rect) -> Option<RgbaImage> {
        let r = r.intersect(&self.area)?;
        let (sx, sy) = self.scale();
        let x0 = (((r.x - self.area.x) as f64) * sx).round() as u32;
        let y0 = (((r.y - self.area.y) as f64) * sy).round() as u32;
        let x1 = ((((r.right() - self.area.x) as f64) * sx).round() as u32).min(self.img.width());
        let y1 = ((((r.bottom() - self.area.y) as f64) * sy).round() as u32).min(self.img.height());
        (x1 > x0 && y1 > y0).then(|| image::imageops::crop_imm(&self.img, x0, y0, x1 - x0, y1 - y0).to_image())
    }

    /// Several monitors' pictures as one, at the densest monitor's scale.
    /// Gaps between monitors (uneven sizes) are opaque black, so the
    /// picture stays opaque (saved as RGB). Reads the parts, copies none.
    pub fn stitch<'a>(parts: impl IntoIterator<Item = &'a Shot>) -> Option<Shot> {
        let parts: Vec<&Shot> = parts.into_iter().collect();
        let area = Rect::bounds(parts.iter().map(|p| p.area))?;
        let s = parts.iter().map(|p| p.scale().0.max(p.scale().1)).fold(1.0f64, f64::max);
        let px = |v: i32| (v as f64 * s).round() as u32;
        let mut canvas = RgbaImage::from_pixel(px(area.w), px(area.h), image::Rgba([0, 0, 0, 255]));
        for p in parts {
            let (w, h) = (px(p.area.w), px(p.area.h));
            let (x, y) = (px(p.area.x - area.x) as i64, px(p.area.y - area.y) as i64);
            if (p.img.width(), p.img.height()) == (w, h) {
                image::imageops::replace(&mut canvas, &p.img, x, y);
            } else {
                image::imageops::replace(&mut canvas, &image::imageops::resize(&p.img, w, h, image::imageops::FilterType::Triangle), x, y);
            }
        }
        Some(Shot { img: canvas, area })
    }

    /// PARTS as one picture; a single part is passed through as is.
    pub fn join(mut parts: Vec<Shot>) -> Option<Shot> {
        if parts.len() == 1 {
            return parts.pop();
        }
        Shot::stitch(&parts)
    }
}

/// R cut from PARTS (one per monitor) at the density of the densest
/// monitor R touches, not of the whole desktop.
pub fn crop_parts(parts: &[Shot], r: Rect) -> Option<RgbaImage> {
    let touched: Vec<&Shot> = parts.iter().filter(|p| p.area.intersect(&r).is_some()).collect();
    match touched[..] {
        [] => None,
        [one] => one.crop(r),
        _ => Shot::stitch(touched)?.crop(r),
    }
}

/// How the bytes of one pixel are laid out (Wayland/DRM fourcc order,
/// little-endian u32), or a QImage-style byte order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fmt {
    /// u32 0xAARRGGBB (wl_shm argb8888/xrgb8888, X11 ZPixmap depth 24/32).
    Xrgb8888 { alpha: bool },
    /// u32 0xAABBGGRR (abgr8888/xbgr8888): bytes R G B A.
    Xbgr8888 { alpha: bool },
    /// u32 2:10:10:10, red in the high bits (argb2101010/xrgb2101010).
    Xrgb2101010 { alpha: bool },
    /// u32 2:10:10:10, blue in the high bits (abgr2101010/xbgr2101010).
    Xbgr2101010 { alpha: bool },
}

impl Fmt {
    /// From a wl_shm / DRM format code; None if we can't read it.
    pub fn from_wl(code: u32) -> Option<Fmt> {
        Some(match code {
            0 => Fmt::Xrgb8888 { alpha: true },  // wl_shm argb8888
            1 => Fmt::Xrgb8888 { alpha: false }, // wl_shm xrgb8888
            0x3432_4241 => Fmt::Xbgr8888 { alpha: true },  // AB24
            0x3432_4258 => Fmt::Xbgr8888 { alpha: false }, // XB24
            0x3033_5241 => Fmt::Xrgb2101010 { alpha: true },  // AR30
            0x3033_5258 => Fmt::Xrgb2101010 { alpha: false }, // XR30
            0x3033_4241 => Fmt::Xbgr2101010 { alpha: true },  // AB30
            0x3033_4258 => Fmt::Xbgr2101010 { alpha: false }, // XB30
            _ => return None,
        })
    }

    /// Prefer 8-bit formats (no conversion loss, the common case), and
    /// among those the ones without alpha.
    pub fn rank(self) -> u8 {
        let (deep, alpha) = match self {
            Fmt::Xrgb8888 { alpha } | Fmt::Xbgr8888 { alpha } => (0, alpha),
            Fmt::Xrgb2101010 { alpha } | Fmt::Xbgr2101010 { alpha } => (1, alpha),
        };
        deep * 2 + alpha as u8
    }

    /// The same layout with the alpha bits ignored. Screen captures are
    /// opaque: whatever an A format carries there is not transparency.
    pub fn opaque(self) -> Fmt {
        match self {
            Fmt::Xrgb8888 { .. } => Fmt::Xrgb8888 { alpha: false },
            Fmt::Xbgr8888 { .. } => Fmt::Xbgr8888 { alpha: false },
            Fmt::Xrgb2101010 { .. } => Fmt::Xrgb2101010 { alpha: false },
            Fmt::Xbgr2101010 { .. } => Fmt::Xbgr2101010 { alpha: false },
        }
    }

    fn rgba(self, b: [u8; 4]) -> [u8; 4] {
        let v = u32::from_le_bytes(b);
        let ten = |s: u32| (((v >> s) & 0x3ff) * 255 / 1023) as u8;
        let a2 = |alpha: bool| if alpha { ((v >> 30) * 85) as u8 } else { 255 };
        match self {
            Fmt::Xrgb8888 { alpha } => [(v >> 16) as u8, (v >> 8) as u8, v as u8, if alpha { (v >> 24) as u8 } else { 255 }],
            Fmt::Xbgr8888 { alpha } => [b[0], b[1], b[2], if alpha { b[3] } else { 255 }],
            Fmt::Xrgb2101010 { alpha } => [ten(20), ten(10), ten(0), a2(alpha)],
            Fmt::Xbgr2101010 { alpha } => [ten(0), ten(10), ten(20), a2(alpha)],
        }
    }
}

/// Raw rows (STRIDE bytes apart, the last one may be short) -> RGBA.
/// Alpha is kept as FMT says (captures pass an opaque() format).
pub fn from_raw(w: u32, h: u32, stride: usize, fmt: Fmt, data: &[u8], y_invert: bool) -> Result<RgbaImage, String> {
    let (wu, hu) = (w as usize, h as usize);
    if w == 0 || h == 0 {
        return Err("empty image".into());
    }
    if stride < wu * 4 {
        return Err(format!("bad stride {stride} for width {w}"));
    }
    let need = stride * (hu - 1) + wu * 4;
    if data.len() < need {
        return Err(format!("got {} bytes, expected {need} ({w}x{h}, stride {stride})", data.len()));
    }
    let mut out = Vec::with_capacity(wu * hu * 4);
    for y in 0..hu {
        let row = if y_invert { hu - 1 - y } else { y };
        let line = &data[row * stride..row * stride + wu * 4];
        for p in line.as_chunks::<4>().0 {
            out.extend_from_slice(&fmt.rgba(*p));
        }
    }
    RgbaImage::from_raw(w, h, out).ok_or_else(|| "bad image size".into())
}

/// wl_output.transform: 0 normal, 1-3 = 90/180/270 degrees, 4-7 = the
/// same with a flip. Turns a buffer in the output's own (unrotated)
/// orientation into what the user sees. Checked against Sway 1.12 for all
/// eight (a bar at the top must stay at the top).
pub fn apply_transform(img: RgbaImage, t: u32) -> RgbaImage {
    use image::imageops::{flip_horizontal, flip_vertical, rotate180, rotate270, rotate90};
    match t & 7 {
        1 => rotate90(&img),
        2 => rotate180(&img),
        3 => rotate270(&img),
        4 => flip_horizontal(&img),
        5 => rotate270(&flip_horizontal(&img)),
        6 => flip_vertical(&img),
        7 => rotate90(&flip_horizontal(&img)),
        _ => img,
    }
}

/// True when every pixel is opaque (then we save RGB: smaller files).
fn opaque(img: &RgbaImage) -> bool {
    img.as_raw().as_chunks::<4>().0.iter().all(|p| p[3] == 255)
}

fn rgb(img: &RgbaImage) -> Vec<u8> {
    img.as_raw().as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect()
}

/// Encode IMG as FMT (png, jpg = quality 90, webp = lossless).
pub fn encode(img: &RgbaImage, fmt: &str, out: &mut impl std::io::Write) -> Result<(), String> {
    use image::ExtendedColorType::{Rgb8, Rgba8};
    let (w, h) = img.dimensions();
    let r = match fmt {
        "jpg" => image::codecs::jpeg::JpegEncoder::new_with_quality(out, 90).write_image(&rgb(img), w, h, Rgb8),
        "webp" => image::codecs::webp::WebPEncoder::new_lossless(out).write_image(img.as_raw(), w, h, Rgba8),
        _ if opaque(img) => image::codecs::png::PngEncoder::new(out).write_image(&rgb(img), w, h, Rgb8),
        _ => image::codecs::png::PngEncoder::new(out).write_image(img.as_raw(), w, h, Rgba8),
    };
    r.map_err(|e| format!("encoding {fmt}: {e}"))
}

/// Save IMG to OUT as FMT via a temp file beside it, so a failure leaves
/// nothing behind.
pub fn save(img: &RgbaImage, fmt: &str, out: &Path) -> Result<(), String> {
    let tmp = out.with_file_name(format!(".{}.part", out.file_name().and_then(|n| n.to_str()).unwrap_or("shot")));
    let r = (|| -> Result<(), String> {
        let f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        let mut w = std::io::BufWriter::new(f);
        encode(img, fmt, &mut w)?;
        let f = w.into_inner().map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, out).map_err(|e| e.to_string())
    })();
    if r.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    r.map_err(|e| format!("{}: {e}", out.display()))
}

/// Read a PNG (or any format we can decode) file into RGBA.
pub fn load(path: &Path) -> Result<RgbaImage, String> {
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    image::load_from_memory(&data).map(|i| i.to_rgba8()).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn img(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| Rgba(f(x, y)))
    }

    #[test]
    fn rects() {
        let a = Rect::new(0, 0, 100, 50);
        assert_eq!(a.intersect(&Rect::new(90, 40, 20, 20)), Some(Rect::new(90, 40, 10, 10)));
        assert_eq!(a.intersect(&Rect::new(100, 0, 5, 5)), None);
        assert_eq!(Rect::bounds([a, Rect::new(-10, 60, 5, 5)]), Some(Rect::new(-10, 0, 110, 65)));
        assert!(a.contains(99, 49) && !a.contains(100, 0));
    }

    #[test]
    fn raw_formats() {
        let px = |fmt, b: [u8; 4]| from_raw(1, 1, 4, fmt, &b, false).unwrap().get_pixel(0, 0).0;
        assert_eq!(px(Fmt::Xrgb8888 { alpha: false }, [0x30, 0x20, 0x10, 0x00]), [0x10, 0x20, 0x30, 255]);
        assert_eq!(px(Fmt::Xrgb8888 { alpha: true }, [0x30, 0x20, 0x10, 0x80]), [0x10, 0x20, 0x30, 0x80]);
        assert_eq!(px(Fmt::Xbgr8888 { alpha: false }, [1, 2, 3, 0]), [1, 2, 3, 255]);
        let v: u32 = (3 << 30) | (1023 << 20) | (512 << 10); // red full, green half, blue 0
        assert_eq!(px(Fmt::Xrgb2101010 { alpha: true }, v.to_le_bytes()), [255, 127, 0, 255]);
        assert_eq!(px(Fmt::Xbgr2101010 { alpha: false }, v.to_le_bytes()), [0, 127, 255, 255]);
        assert_eq!(Fmt::from_wl(1), Some(Fmt::Xrgb8888 { alpha: false }));
        assert_eq!(Fmt::from_wl(0x3033_5258), Some(Fmt::Xrgb2101010 { alpha: false }));
        assert_eq!(Fmt::from_wl(42), None);
        assert_eq!(px(Fmt::from_wl(0).unwrap().opaque(), [0x30, 0x20, 0x10, 0x00]), [0x10, 0x20, 0x30, 255]);
        assert!(Fmt::from_wl(1).unwrap().rank() < Fmt::from_wl(0).unwrap().rank());
    }

    #[test]
    fn raw_stride_invert_and_errors() {
        // 1x2, stride 8, last row short; y_invert swaps the rows.
        let data = [1, 0, 0, 0, 9, 9, 9, 9, 2, 0, 0, 0];
        let f = Fmt::Xrgb8888 { alpha: false };
        let i = from_raw(1, 2, 8, f, &data, true).unwrap();
        assert_eq!((i.get_pixel(0, 0).0[2], i.get_pixel(0, 1).0[2]), (2, 1));
        assert!(from_raw(2, 2, 8, f, &[0; 12], false).unwrap_err().contains("expected 16"));
        assert!(from_raw(2, 1, 4, f, &[0; 8], false).unwrap_err().contains("bad stride"));
        assert!(from_raw(0, 1, 4, f, &[], false).is_err());
    }

    #[test]
    fn transforms() {
        // 2x1: red, green. Transform 1 (90°): the buffer is 2 wide, the
        // user sees it 1 wide and 2 tall.
        let i = img(2, 1, |x, _| if x == 0 { [255, 0, 0, 255] } else { [0, 255, 0, 255] });
        let r = apply_transform(i.clone(), 1);
        assert_eq!(r.dimensions(), (1, 2));
        assert_eq!((r.get_pixel(0, 0).0[0], r.get_pixel(0, 1).0[1]), (255, 255));
        let r = apply_transform(i.clone(), 3);
        assert_eq!((r.get_pixel(0, 0).0[1], r.get_pixel(0, 1).0[0]), (255, 255));
        assert_eq!(apply_transform(i.clone(), 2).get_pixel(0, 0).0[1], 255);
        assert_eq!(apply_transform(i.clone(), 4).get_pixel(0, 0).0[1], 255);
        assert_eq!(apply_transform(i.clone(), 0), i);
    }

    #[test]
    fn stitch_and_crop() {
        // Two monitors side by side: A 100x50 at scale 1, B 100x50 at scale 2.
        let a = Shot { img: img(100, 50, |_, _| [255, 0, 0, 255]), area: Rect::new(0, 0, 100, 50) };
        let b = Shot { img: img(200, 100, |_, _| [0, 0, 255, 255]), area: Rect::new(100, 0, 100, 50) };
        let s = Shot::stitch([&a, &b]).unwrap();
        assert_eq!((s.img.dimensions(), s.area), ((400, 100), Rect::new(0, 0, 200, 50)));
        assert_eq!(s.img.get_pixel(10, 10).0, [255, 0, 0, 255]);
        assert_eq!(s.img.get_pixel(390, 90).0, [0, 0, 255, 255]);
        let c = s.crop(Rect::new(90, 10, 20, 10)).unwrap();
        assert_eq!(c.dimensions(), (40, 20));
        assert_eq!((c.get_pixel(0, 0).0[0], c.get_pixel(39, 0).0[2]), (255, 255));
        assert!(s.crop(Rect::new(500, 0, 10, 10)).is_none());
        // Off-desktop parts are clipped.
        assert_eq!(s.crop(Rect::new(-10, -10, 20, 20)).unwrap().dimensions(), (20, 20));
        // A region on the 1x monitor alone keeps 1x density.
        let parts = vec![
            Shot { img: img(100, 50, |_, _| [255, 0, 0, 255]), area: Rect::new(0, 0, 100, 50) },
            Shot { img: img(200, 100, |_, _| [0, 0, 255, 255]), area: Rect::new(100, 0, 100, 50) },
        ];
        assert_eq!(crop_parts(&parts, Rect::new(10, 10, 20, 10)).unwrap().dimensions(), (20, 10));
        assert_eq!(crop_parts(&parts, Rect::new(90, 10, 20, 10)).unwrap().dimensions(), (40, 20));
        // A gap (monitors not touching) is opaque black.
        let a = Shot { img: img(10, 10, |_, _| [1, 1, 1, 255]), area: Rect::new(0, 0, 10, 10) };
        let b = Shot { img: img(10, 10, |_, _| [2, 2, 2, 255]), area: Rect::new(10, 10, 10, 10) };
        let s = Shot::stitch([&a, &b]).unwrap();
        assert_eq!(s.img.get_pixel(15, 2).0, [0, 0, 0, 255]);
        assert!(opaque(&s.img));
        // join passes a single part through; crop_parts of nothing is None.
        assert_eq!(Shot::join(vec![a]).unwrap().area, Rect::new(0, 0, 10, 10));
        assert!(crop_parts(&parts, Rect::new(500, 0, 5, 5)).is_none());
    }

    #[test]
    fn encode_formats() {
        let d = crate::testutil::tempdir("pixels");
        let i = img(7, 5, |x, y| [(x * 30) as u8, (y * 40) as u8, 99, 255]);
        for (fmt, magic) in [("png", &b"\x89PNG"[..]), ("jpg", &b"\xff\xd8\xff"[..]), ("webp", &b"RIFF"[..])] {
            let f = d.join(format!("a.{fmt}"));
            save(&i, fmt, &f).unwrap();
            let data = std::fs::read(&f).unwrap();
            assert!(data.starts_with(magic), "{fmt}");
            let back = load(&f).unwrap();
            assert_eq!(back.dimensions(), (7, 5));
            if fmt != "jpg" {
                assert_eq!(back, i, "{fmt} is lossless");
            }
        }
        // Opaque PNGs are stored as RGB; transparent ones keep alpha.
        let t = img(2, 2, |_, _| [1, 2, 3, 4]);
        save(&t, "png", &d.join("t.png")).unwrap();
        assert_eq!(load(&d.join("t.png")).unwrap(), t);
        let names: Vec<_> = std::fs::read_dir(&*d).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        assert!(!names.iter().any(|n| n.ends_with(".part")), "{names:?}");
        assert!(save(&i, "png", &d.join("no/such/dir.png")).is_err());
    }
}
