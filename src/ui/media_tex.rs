//! Your pictures and GIFs as textures, crisp (nearest), made once each. A GIF file's frames are
//! put together the way a browser plays them (each frame may change only part of the picture),
//! with a browser's timing: a delay of 10 ms or less plays at 100 ms. A drawn GIF shows each frame
//! for 1000 / fps ms, slower or faster with its look's speed, as in the Minniondeck app.
use std::cell::RefCell;
use std::collections::HashMap;

use egui::{ColorImage, Context, TextureHandle, TextureId, TextureOptions};

use crate::backend::media::{self, GifSource};

/// The most frames kept of one GIF
const MAX_FRAMES: usize = 300;
/// A picture is shrunk to at most this many pixels a side for drawing
const MAX_SIDE: u32 = 512;
/// A GIF's frames are shrunk to this a side, and all its frames together hold at most this many
/// pixels (a 512 x 512 GIF keeps about 120 frames): a huge GIF can't take all the memory
const GIF_SIDE: u32 = 512;
const GIF_PIXELS: usize = 32 * 1024 * 1024;

struct Anim {
    frames: Vec<TextureHandle>,
    /// How long each frame shows, in seconds
    delays: Vec<f64>,
    total: f64,
    size: [usize; 2],
    /// Speed applies to drawn GIFs only (a file GIF keeps its own timing, as in the app)
    drawn: bool,
}

thread_local! {
    /// Each made texture with what it was made from (its source's key), so a change in the lists
    /// forgets only the ones whose source changed
    static PICS: RefCell<HashMap<String, (u64, Option<(TextureHandle, [usize; 2])>)>> = RefCell::new(HashMap::new());
    static GIFS: RefCell<HashMap<String, (u64, Option<Anim>)>> = RefCell::new(HashMap::new());
    static STAMP: RefCell<u64> = const { RefCell::new(0) };
}

fn key_of<T: std::hash::Hash>(v: &T) -> u64 {
    use std::hash::Hasher;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

/// What a picture's texture is made from: its file (0: none)
fn picture_key(id: &str) -> u64 {
    media::picture(id).and_then(|p| p.file).map(|f| key_of(&f)).unwrap_or(0)
}

/// What a GIF's frames are made from: its file, or the drawing itself
fn gif_key(id: &str) -> u64 {
    match media::gif(id).and_then(|g| g.src) {
        Some(GifSource::File(p)) => key_of(&p),
        Some(GifSource::Drawn { size, fps, frames }) => key_of(&(size, fps.to_bits(), frames)),
        None => 0,
    }
}

/// When the lists changed (a picture imported, a library restored), forgets only the textures
/// whose source isn't the same any more; the rest stay made.
fn check_stamp() {
    let now = media::stamp();
    STAMP.with(|s| {
        if *s.borrow() != now {
            *s.borrow_mut() = now;
            PICS.with(|p| p.borrow_mut().retain(|id, (k, _)| *k == picture_key(id)));
            GIFS.with(|g| g.borrow_mut().retain(|id, (k, _)| *k == gif_key(id)));
        }
    });
}

fn to_color_image(img: &image::RgbaImage) -> ColorImage {
    ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw())
}

/// A picture of the library: its texture and size in pixels.
pub fn picture(ctx: &Context, id: &str) -> Option<(TextureId, [usize; 2])> {
    check_stamp();
    PICS.with(|p| {
        let mut p = p.borrow_mut();
        let e = p.entry(id.to_string()).or_insert_with(|| {
            let made = (|| {
                let file = media::picture(id)?.file?;
                let img = media::decode_limited(&media::read_bytes(&file)?).ok()?;
                let img = if img.width() > MAX_SIDE || img.height() > MAX_SIDE { img.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::Triangle) } else { img };
                let rgba = img.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                Some((ctx.load_texture(format!("pic:{id}"), to_color_image(&rgba), TextureOptions::NEAREST), size))
            })();
            (picture_key(id), made)
        });
        e.1.as_ref().map(|(t, s)| (t.id(), *s))
    })
}

fn load_gif(ctx: &Context, id: &str) -> Option<Anim> {
    let g = media::gif(id)?;
    match g.src? {
        GifSource::Drawn { size, fps, frames } => {
            let mut out = vec![];
            for (n, f) in frames.iter().take(MAX_FRAMES).enumerate() {
                let mut img = ColorImage::new([size, size], vec![egui::Color32::TRANSPARENT; size * size]);
                for (i, px) in f.iter().enumerate() {
                    if let Some([r, g, b, a]) = px {
                        img.pixels[i] = egui::Color32::from_rgba_unmultiplied(*r, *g, *b, *a);
                    }
                }
                out.push(ctx.load_texture(format!("gif:{id}:{n}"), img, TextureOptions::NEAREST));
            }
            let d = 1.0 / fps as f64;
            let delays = vec![d; out.len()];
            Some(Anim { total: d * out.len() as f64, frames: out, delays, size: [size, size], drawn: true })
        }
        GifSource::File(path) => {
            use image::{AnimationDecoder, ImageDecoder};
            let file = std::io::Cursor::new(media::read_bytes(&path)?);
            let dec = image::codecs::gif::GifDecoder::new(file).ok()?;
            // a GIF that claims to be enormous isn't decoded at all
            let (w, h) = dec.dimensions();
            if w == 0 || h == 0 || w > media::MAX_SIDE || h > media::MAX_SIDE {
                return None;
            }
            let mut frames = vec![];
            let mut delays = vec![];
            let mut size = [1usize, 1usize];
            let mut pixels = 0usize;
            for (n, f) in dec.into_frames().take(MAX_FRAMES).enumerate() {
                let Ok(f) = f else { break };
                let (num, den) = f.delay().numer_denom_ms();
                let ms = if den == 0 { 100.0 } else { num as f64 / den as f64 };
                let mut buf = f.into_buffer();
                if buf.width() > GIF_SIDE || buf.height() > GIF_SIDE {
                    buf = image::DynamicImage::ImageRgba8(buf).resize(GIF_SIDE, GIF_SIDE, image::imageops::FilterType::Triangle).to_rgba8();
                }
                pixels += (buf.width() * buf.height()) as usize;
                if pixels > GIF_PIXELS && !frames.is_empty() {
                    break;
                }
                delays.push(if ms <= 10.0 { 0.1 } else { ms / 1000.0 });
                size = [buf.width() as usize, buf.height() as usize];
                frames.push(ctx.load_texture(format!("gif:{id}:{n}"), to_color_image(&buf), TextureOptions::NEAREST));
            }
            if frames.is_empty() {
                return None;
            }
            Some(Anim { total: delays.iter().sum(), frames, delays, size, drawn: false })
        }
    }
}

/// A GIF's frame at time `t` (seconds), with `speed` 0 slow, 1 normal, 2 fast: its texture, its
/// size in pixels, and how long until the next frame (None for a GIF of one frame).
pub fn gif_frame(ctx: &Context, id: &str, t: f64, speed: u8) -> Option<(TextureId, [usize; 2], Option<f64>)> {
    check_stamp();
    GIFS.with(|g| {
        let mut g = g.borrow_mut();
        let a = g.entry(id.to_string()).or_insert_with(|| (gif_key(id), load_gif(ctx, id))).1.as_ref()?;
        if a.frames.len() == 1 || a.total <= 0.0 {
            return Some((a.frames[0].id(), a.size, None));
        }
        // the app's SPEED: slow 1.6, normal 1, fast 0.6
        let k = if a.drawn { [1.6, 1.0, 0.6][speed.min(2) as usize] } else { 1.0 };
        let mut at = (t / k).rem_euclid(a.total);
        for (i, d) in a.delays.iter().enumerate() {
            if at < *d {
                return Some((a.frames[i].id(), a.size, Some((d - at) * k)));
            }
            at -= d;
        }
        Some((a.frames[0].id(), a.size, Some(0.05)))
    })
}

/// The part of a picture (as uv) that fills a box of `bw` × `bh` without stretching ("Fill the
/// button"): the middle, cut to the box's shape.
pub fn cover_uv(size: [usize; 2], bw: f32, bh: f32) -> egui::Rect {
    let (iw, ih) = (size[0].max(1) as f32, size[1].max(1) as f32);
    let (ia, ba) = (iw / ih, bw / bh.max(1.0));
    if ia > ba {
        let u = ba / ia;
        egui::Rect::from_min_max(egui::pos2((1.0 - u) / 2.0, 0.0), egui::pos2((1.0 + u) / 2.0, 1.0))
    } else {
        let v = ia / ba;
        egui::Rect::from_min_max(egui::pos2(0.0, (1.0 - v) / 2.0), egui::pos2(1.0, (1.0 + v) / 2.0))
    }
}

/// Where a picture goes to show all of it in a box ("Show it all"): (x, y, w, h) inside the box.
pub fn contain_box(size: [usize; 2], bw: f32, bh: f32) -> (f32, f32, f32, f32) {
    let (iw, ih) = (size[0].max(1) as f32, size[1].max(1) as f32);
    let k = (bw / iw).min(bh / ih);
    let (w, h) = (iw * k, ih * k);
    ((bw - w) / 2.0, (bh - h) / 2.0, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gif_file_plays_its_frames_with_a_browsers_timing() {
        let _one = crate::backend::media::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        use image::codecs::gif::GifEncoder;
        use image::{Delay, Frame, Rgba, RgbaImage};
        let dir = std::env::temp_dir().join(format!("md-giftex-{}", crate::backend::util::now_ms()));
        let media_dir = dir.join("media");
        std::fs::create_dir_all(&media_dir).unwrap();
        let mut bytes = vec![];
        {
            let mut enc = GifEncoder::new(&mut bytes);
            let red = RgbaImage::from_pixel(4, 4, Rgba([255, 0, 0, 255]));
            let blue = RgbaImage::from_pixel(4, 4, Rgba([0, 0, 255, 255]));
            enc.encode_frame(Frame::from_parts(red, 0, 0, Delay::from_numer_denom_ms(200, 1))).unwrap();
            // 0 ms plays at 100 ms, as in a browser
            enc.encode_frame(Frame::from_parts(blue, 0, 0, Delay::from_numer_denom_ms(0, 1))).unwrap();
        }
        media::store(&media_dir, "f1-abc.gif", &bytes).unwrap();
        let doc = serde_json::json!({ "gifFiles": [{ "id": "f1", "name": "Blink", "media": "f1-abc.gif" }],
            "drawnGifs": [{ "id": "g1", "name": "Dot", "size": 1, "fps": 10, "frames": [["#ffffff"], ["#000000"]] }] });
        media::set_from_doc(&doc, &media_dir, &dir.join("plugins"));
        let ctx = egui::Context::default();
        let (a, size, next) = gif_frame(&ctx, "f1", 0.05, 1).unwrap();
        assert_eq!(size, [4, 4]);
        assert!((next.unwrap() - 0.15).abs() < 1e-6, "first frame shows 200 ms");
        let (b, _, next) = gif_frame(&ctx, "f1", 0.25, 1).unwrap();
        assert_ne!(a, b, "the second frame after 200 ms");
        assert!((next.unwrap() - 0.05).abs() < 1e-6, "a 0 ms frame plays at 100 ms");
        assert_eq!(gif_frame(&ctx, "f1", 0.30 + 0.05, 2).unwrap().0, a, "it loops; speed doesn't change a file GIF");
        // a drawn GIF: 1000 / fps ms a frame, 1.6 times slower on Slow
        let (d0, _, n0) = gif_frame(&ctx, "g1", 0.0, 1).unwrap();
        assert!((n0.unwrap() - 0.1).abs() < 1e-6);
        assert_ne!(gif_frame(&ctx, "g1", 0.12, 1).unwrap().0, d0);
        assert_eq!(gif_frame(&ctx, "g1", 0.12, 0).unwrap().0, d0, "slow: still the first frame");
        assert!(gif_frame(&ctx, "nope", 0.0, 1).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // a change in the lists keeps the textures whose source is the same
    #[test]
    fn only_textures_whose_source_changed_are_made_again() {
        let _one = crate::backend::media::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("md-tex-keep-{}", crate::backend::util::now_ms()));
        let media_dir = dir.join("media");
        media::store(&media_dir, "a.png", &media::tiny_png()).unwrap();
        media::store(&media_dir, "b.png", &media::tiny_png()).unwrap();
        let doc = |b: &str| serde_json::json!({ "images": [{ "id": "u:a", "media": "a.png" }, { "id": "u:b", "media": b }] });
        media::set_from_doc(&doc("b.png"), &media_dir, &dir.join("plugins"));
        let ctx = egui::Context::default();
        let a = picture(&ctx, "u:a").unwrap().0;
        let b = picture(&ctx, "u:b").unwrap().0;
        media::store(&media_dir, "c.png", &media::tiny_png()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        media::set_from_doc(&doc("c.png"), &media_dir, &dir.join("plugins"));
        assert_eq!(picture(&ctx, "u:a").unwrap().0, a, "unchanged: kept");
        assert_ne!(picture(&ctx, "u:b").unwrap().0, b, "another file: made again");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_wide_picture_is_cut_at_the_sides_to_fill_a_square() {
        let uv = cover_uv([200, 100], 50.0, 50.0);
        assert!((uv.min.x - 0.25).abs() < 1e-4 && (uv.max.x - 0.75).abs() < 1e-4);
        assert_eq!((uv.min.y, uv.max.y), (0.0, 1.0));
        let (x, y, w, h) = contain_box([200, 100], 50.0, 50.0);
        assert_eq!((x, y, w, h), (0.0, 12.5, 50.0, 25.0));
    }
}
