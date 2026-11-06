//! Pictures for the phone page, made on the PC: any icon as a small PNG in a given colour (the
//! page draws it crisp), the app's icon, and a GIF drawn in the GIF maker as a GIF file.
use crate::ui::icons;

fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().ok()?;
        writer.write_image_data(rgba).ok()?;
    }
    Some(out)
}

/// A one-colour bitmap: `rows` of `n` bits each (the top bit is the left pixel).
fn bitmap(rows: &[u32], n: usize, ink: [u8; 4]) -> Option<Vec<u8>> {
    let mut px = vec![0u8; n * n * 4];
    for (y, row) in rows.iter().take(n).enumerate() {
        for x in 0..n {
            if row >> (n - 1 - x) & 1 == 1 {
                px[(y * n + x) * 4..(y * n + x) * 4 + 4].copy_from_slice(&ink);
            }
        }
    }
    encode_png(n as u32, n as u32, &px)
}

/// An icon id ("px:name", "si:slug", "png:plugin/name" or a glyph name) as PNG bytes, in `ink`
/// for the one-colour ones. None when the app doesn't know it.
pub fn icon_png(id: &str, ink: [u8; 4]) -> Option<Vec<u8>> {
    if let Some(key) = id.strip_prefix("png:") {
        // a plugin's picture as it is: built in, or one a plugin put on a button
        return crate::generated::catalog::png(key).map(|b| b.to_vec()).or_else(|| crate::ui::images::live_bytes(key));
    }
    if let Some(name) = id.strip_prefix("px:") {
        return bitmap(icons::px_icons().get(name)?, 24, ink);
    }
    if let Some(slug) = id.strip_prefix("si:") {
        if let Some(a) = icons::brand_art().get(slug) {
            let mut px = vec![0u8; 16 * 16 * 4];
            for (y, row) in a.rows.iter().take(16).enumerate() {
                for (x, ch) in row.chars().take(16).enumerate() {
                    if let Some((_, c)) = a.palette.iter().find(|(p, _)| *p == ch) {
                        px[(y * 16 + x) * 4..(y * 16 + x) * 4 + 4].copy_from_slice(&[(*c >> 16) as u8, (*c >> 8) as u8, *c as u8, 255]);
                    }
                }
            }
            return encode_png(16, 16, &px);
        }
        let i = *icons::si_by_slug().get(slug)?;
        return bitmap(&icons::si_icons()[i].rows, 24, ink);
    }
    if let Some(rows) = icons::glyphs().get(id) {
        let bits: Vec<u32> = rows.iter().map(|r| r.chars().fold(0u32, |a, c| a << 1 | (c == '#') as u32)).collect();
        return bitmap(&bits, 8, ink);
    }
    bitmap(icons::px_icons().get(id)?, 24, ink)
}

/// The app's icon for the phone's home screen: the minnion on the app's dark colour.
pub fn app_icon_png(size: u32) -> Option<Vec<u8>> {
    let minnion = crate::backend::tray::pixels(size, true, 0);
    let mut px = vec![0u8; (size * size * 4) as usize];
    for (i, p) in px.chunks_mut(4).enumerate() {
        p.copy_from_slice(&[0x1e, 0x24, 0x33, 255]);
        let m = &minnion[i * 4..i * 4 + 4];
        if m[3] > 0 {
            p.copy_from_slice(m);
        }
    }
    encode_png(size, size, &px)
}

/// A GIF drawn in the GIF maker (size × size pixels, `fps` frames a second) as a GIF file.
pub fn drawn_gif(size: usize, fps: f32, frames: &[Vec<Option<[u8; 4]>>]) -> Option<Vec<u8>> {
    use image::codecs::gif::{GifEncoder, Repeat};
    use image::{Delay, Frame, RgbaImage};
    let mut out = Vec::new();
    {
        let mut enc = GifEncoder::new(&mut out);
        enc.set_repeat(Repeat::Infinite).ok()?;
        let ms = (1000.0 / fps.max(0.5)).round() as u32;
        for f in frames {
            let mut img = RgbaImage::new(size as u32, size as u32);
            for (i, px) in f.iter().enumerate().take(size * size) {
                if let Some(c) = px {
                    img.put_pixel((i % size) as u32, (i / size) as u32, image::Rgba(*c));
                }
            }
            enc.encode_frame(Frame::from_parts(img, 0, 0, Delay::from_numer_denom_ms(ms, 1))).ok()?;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size_of(png_bytes: &[u8]) -> (u32, u32) {
        let d = png::Decoder::new(png_bytes);
        let r = d.read_info().unwrap();
        (r.info().width, r.info().height)
    }

    #[test]
    fn icons_become_pngs_in_the_asked_colour() {
        let g = icon_png("play", [255, 0, 0, 255]).expect("a glyph");
        assert_eq!(size_of(&g), (8, 8));
        let p = icon_png("px:home", [0, 0, 255, 255]).or_else(|| icon_png(icons::px_names()[0], [0, 0, 255, 255])).expect("a pixel icon");
        assert_eq!(size_of(&p), (24, 24));
        // the same icon in another colour is another picture
        assert_ne!(icon_png("play", [255, 0, 0, 255]), icon_png("play", [0, 255, 0, 255]));
        // a logo with colour art is 16 px; one without is a 24 px one-colour icon
        if let Some(slug) = icons::brand_art().keys().next() {
            assert_eq!(size_of(&icon_png(&format!("si:{slug}"), [0, 0, 0, 255]).unwrap()), (16, 16));
        }
        assert!(icon_png("no-such-icon-anywhere", [0, 0, 0, 255]).is_none());
        assert_eq!(size_of(&app_icon_png(192).unwrap()), (192, 192));
    }

    #[test]
    fn a_drawn_gif_becomes_a_gif_file() {
        let red = Some([255, 0, 0, 255]);
        let bytes = drawn_gif(2, 4.0, &[vec![red, None, None, red], vec![None, red, red, None]]).unwrap();
        assert!(bytes.starts_with(b"GIF89a") || bytes.starts_with(b"GIF87a"));
        assert_eq!(image::guess_format(&bytes).unwrap(), image::ImageFormat::Gif);
    }
}
