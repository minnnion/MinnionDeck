//! Embedded PNG pictures (plugin icons), decoded once into crisp (nearest) textures.
use egui::{ColorImage, Context, TextureHandle, TextureOptions};
use std::cell::RefCell;
use std::collections::HashMap;

macro_rules! pics {
    ($($name:literal),* $(,)?) => {
        fn bytes(name: &str) -> Option<&'static [u8]> {
            match name {
                $($name => Some(include_bytes!(concat!("../../assets/plugins/", $name, ".png"))),)*
                _ => None,
            }
        }
    };
}

pics!(
    "soundboard", "spotify", "sysmon", "timer", "weather",
    "sp-now", "sp-prev", "sp-play", "sp-pause", "sp-next", "sp-like-off", "sp-like-on", "sp-volume",
    "sys-cpu", "sys-ram", "sys-disk", "sys-net", "sys-gpu", "sys-top",
    "tim-countdown", "tim-stopwatch", "tim-focus", "tim-clock",
    "wx-weather", "wx-sun", "wx-cloud", "wx-rain", "wx-snow", "wx-storm", "wx-fog", "wx-thermo",
);

/// Pictures a plugin puts on a button while it runs, by the name the button gives them.
static LIVE: std::sync::Mutex<Option<HashMap<String, Vec<u8>>>> = std::sync::Mutex::new(None);

pub fn register_live(name: &str, data: Vec<u8>) {
    LIVE.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(name.to_string(), data);
    CACHE.with(|c| c.borrow_mut().remove(name));
}

/// Whether a picture was registered by register_live.
pub fn has_live(name: &str) -> bool {
    live_bytes(name).is_some()
}

pub fn live_bytes(name: &str) -> Option<Vec<u8>> {
    LIVE.lock().unwrap_or_else(|e| e.into_inner()).as_ref()?.get(name).cloned()
}

thread_local! {
    static CACHE: RefCell<HashMap<String, Option<TextureHandle>>> = RefCell::new(HashMap::new());
}

fn decode(data: &[u8]) -> Option<ColorImage> {
    let mut dec = png::Decoder::new(data);
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as usize, info.height as usize);
    let px = &buf[..info.buffer_size()];
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => px.to_vec(),
        png::ColorType::Rgb => px.chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => px.chunks(2).flat_map(|c| [c[0], c[0], c[0], c[1]]).collect(),
        png::ColorType::Grayscale => px.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        _ => return None,
    };
    Some(ColorImage::from_rgba_unmultiplied([w, h], &rgba))
}

pub fn texture(ctx: &Context, name: &str) -> Option<egui::TextureId> {
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        let entry = c.entry(name.to_string()).or_insert_with(|| {
            let img = match bytes(name).or_else(|| crate::generated::catalog::png(name)) {
                Some(b) => decode(b)?,
                None => decode(&live_bytes(name)?)?,
            };
            Some(ctx.load_texture(name, img, TextureOptions::NEAREST))
        });
        entry.as_ref().map(|t| t.id())
    })
}

thread_local! {
    static DOTS: RefCell<HashMap<(u32, u32), TextureHandle>> = RefCell::new(HashMap::new());
}

/// A 16 x 16 tile of the ground with its dot (radial-gradient(dot 2px, transparent 2px)),
/// set to repeat, one per theme.
pub fn dot_tile(ctx: &Context, base: egui::Color32, dot: egui::Color32) -> egui::TextureId {
    let key = (u32::from_be_bytes(base.to_array()), u32::from_be_bytes(dot.to_array()));
    DOTS.with(|m| {
        let mut m = m.borrow_mut();
        m.entry(key)
            .or_insert_with(|| {
                let mut img = ColorImage::new([16, 16], vec![base; 256]);
                for (x0, y0, w, h) in [(7, 6, 2, 1), (6, 7, 4, 2), (7, 9, 2, 1)] {
                    for yy in y0..y0 + h {
                        for xx in x0..x0 + w {
                            img.pixels[yy * 16 + xx] = dot;
                        }
                    }
                }
                let opts = TextureOptions { wrap_mode: egui::TextureWrapMode::Repeat, ..TextureOptions::NEAREST };
                ctx.load_texture(format!("dots{key:?}"), img, opts)
            })
            .id()
    })
}
