//! Icons from gui/icons/png (made by gui/icons/render.sh), built in.

macro_rules! sizes {
    ($name:literal) => {
        &[
            include_bytes!(concat!("../icons/png/", $name, "-16.png")).as_slice(),
            include_bytes!(concat!("../icons/png/", $name, "-22.png")).as_slice(),
            include_bytes!(concat!("../icons/png/", $name, "-24.png")).as_slice(),
            include_bytes!(concat!("../icons/png/", $name, "-32.png")).as_slice(),
            include_bytes!(concat!("../icons/png/", $name, "-48.png")).as_slice(),
            include_bytes!(concat!("../icons/png/", $name, "-64.png")).as_slice(),
        ]
    };
}

pub const IDLE: &[&[u8]] = sizes!("clipped-idle");
pub const REPLAY: &[&[u8]] = sizes!("clipped-replay");
pub const RECORDING: &[&[u8]] = sizes!("clipped-recording");
pub const APP_64: &[u8] = include_bytes!("../icons/png/clipped-64.png");

/// Decode a PNG to (width, height, RGBA8).
pub fn rgba(data: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(data));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut r = dec.read_info().ok()?;
    let mut buf = vec![0; r.output_buffer_size()?];
    let info = r.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    let px: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf.as_chunks::<3>().0.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf.as_chunks::<2>().0.iter().flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return None, // expanded by normalize_to_color8
    };
    Some((info.width, info.height, px))
}

/// StatusNotifierItem pixmaps: ARGB32 in network byte order.
pub fn pixmaps(set: &[&[u8]]) -> Vec<ksni::Icon> {
    set.iter()
        .filter_map(|d| rgba(d))
        .map(|(w, h, mut px)| {
            for p in px.as_chunks_mut::<4>().0 {
                p.rotate_right(1); // RGBA -> ARGB
            }
            ksni::Icon { width: w as i32, height: h as i32, data: px }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_icons_decode_at_their_size() {
        for set in [IDLE, REPLAY, RECORDING] {
            let icons = pixmaps(set);
            let sizes: Vec<i32> = icons.iter().map(|i| i.width).collect();
            assert_eq!(sizes, [16, 22, 24, 32, 48, 64]);
            for i in &icons {
                assert_eq!(i.width, i.height);
                assert_eq!(i.data.len(), (i.width * i.height * 4) as usize);
            }
        }
        assert_eq!(rgba(APP_64).map(|(w, h, _)| (w, h)), Some((64, 64)));
    }
}
