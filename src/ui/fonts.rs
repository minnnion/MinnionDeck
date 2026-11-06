//! Pixelify Sans, one static face per weight (the web UI uses the variable font at 400-700).
use egui::{Context, FontData, FontDefinitions, FontFamily, FontId};
use std::sync::Arc;

const F400: &[u8] = include_bytes!("../../assets/fonts/PixelifySans-400.ttf");
const F500: &[u8] = include_bytes!("../../assets/fonts/PixelifySans-500.ttf");
const F600: &[u8] = include_bytes!("../../assets/fonts/PixelifySans-600.ttf");
const F700: &[u8] = include_bytes!("../../assets/fonts/PixelifySans-700.ttf");

pub fn install(ctx: &Context) {
    let mut fd = FontDefinitions::default();
    for (name, bytes) in [("px400", F400), ("px500", F500), ("px600", F600), ("px700", F700)] {
        fd.font_data.insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
        fd.families.insert(FontFamily::Name(name.into()), vec![name.to_owned()]);
    }
    ctx.set_fonts(fd);
}

pub fn family(weight: u16) -> FontFamily {
    FontFamily::Name(
        match weight {
            0..=450 => "px400",
            451..=550 => "px500",
            551..=650 => "px600",
            _ => "px700",
        }
        .into(),
    )
}

pub fn font(size: f32, weight: u16) -> FontId {
    FontId::new(size, family(weight))
}

/// hhea ascender / descender / unitsPerEm, read straight from the font file.
pub fn vmetrics() -> (f32, f32, f32) {
    let b = F400;
    let u16at = |o: usize| u16::from_be_bytes([b[o], b[o + 1]]);
    let i16at = |o: usize| i16::from_be_bytes([b[o], b[o + 1]]);
    let n = u16at(4) as usize;
    let (mut head, mut hhea) = (0, 0);
    for i in 0..n {
        let o = 12 + i * 16;
        let tag = &b[o..o + 4];
        let off = u32::from_be_bytes([b[o + 8], b[o + 9], b[o + 10], b[o + 11]]) as usize;
        if tag == b"head" { head = off }
        if tag == b"hhea" { hhea = off }
    }
    (i16at(hhea + 4) as f32, -(i16at(hhea + 6) as f32), u16at(head + 18) as f32)
}
