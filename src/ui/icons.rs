//! Pixel icons: 24x24 "pixelarticons" (3 bytes per row, top bit on the left) and the 8x8 glyphs.
use std::collections::HashMap;
use std::sync::OnceLock;

const PX_DATA: &str = include_str!("../../assets/pixel_icons.txt");
const GLYPH_DATA: &str = include_str!("../../assets/glyphs.txt");

fn b64(s: &str) -> Vec<u8> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => 0,
    };
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes().filter(|c| *c != b'=') {
        acc = (acc << 6) | val(c) as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    out
}

pub fn px_icons() -> &'static HashMap<&'static str, [u32; 24]> {
    static M: OnceLock<HashMap<&'static str, [u32; 24]>> = OnceLock::new();
    M.get_or_init(|| {
        let mut m = HashMap::new();
        for item in PX_DATA.split(';') {
            let Some((name, data)) = item.split_once(':') else { continue };
            let bytes = b64(data);
            let mut rows = [0u32; 24];
            for (y, row) in rows.iter_mut().enumerate() {
                let o = y * 3;
                if o + 2 < bytes.len() {
                    *row = (bytes[o] as u32) << 16 | (bytes[o + 1] as u32) << 8 | bytes[o + 2] as u32;
                }
            }
            m.insert(name, rows);
        }
        m
    })
}

pub fn glyphs() -> &'static HashMap<&'static str, Vec<&'static str>> {
    static M: OnceLock<HashMap<&'static str, Vec<&'static str>>> = OnceLock::new();
    M.get_or_init(|| {
        GLYPH_DATA
            .lines()
            .filter_map(|l| l.split_once(':'))
            .map(|(k, v)| (k, v.split(',').collect()))
            .collect()
    })
}

const SI_DATA: &str = include_str!("../../assets/si_icons.txt");
const ART_DATA: &str = include_str!("../../assets/brand_art.txt");

// Every field of the logo data is read in; `colour` isn't drawn yet
#[allow(dead_code)]
pub struct SiIcon {
    pub slug: &'static str,
    pub name: &'static str,
    pub colour: u32,
    pub rows: [u32; 24],
}

/// App and game logos (Simple Icons) as 24x24 bitmaps, in file order.
pub fn si_icons() -> &'static Vec<SiIcon> {
    static V: OnceLock<Vec<SiIcon>> = OnceLock::new();
    V.get_or_init(|| {
        SI_DATA
            .lines()
            .filter_map(|l| {
                let mut p = l.splitn(4, '|');
                let (slug, name, col, data) = (p.next()?, p.next()?, p.next()?, p.next()?);
                let bytes = b64(data);
                let mut rows = [0u32; 24];
                for (y, row) in rows.iter_mut().enumerate() {
                    let o = y * 3;
                    if o + 2 < bytes.len() {
                        *row = (bytes[o] as u32) << 16 | (bytes[o + 1] as u32) << 8 | bytes[o + 2] as u32;
                    }
                }
                Some(SiIcon { slug, name, colour: u32::from_str_radix(col, 16).unwrap_or(0), rows })
            })
            .collect()
    })
}

pub fn si_by_slug() -> &'static HashMap<&'static str, usize> {
    static M: OnceLock<HashMap<&'static str, usize>> = OnceLock::new();
    M.get_or_init(|| si_icons().iter().enumerate().map(|(i, s)| (s.slug, i)).collect())
}

/// Hand-made colour logos: 16x16 pixels, a palette and the colour of the tile they sit on.
#[allow(dead_code)]
pub struct Art {
    pub bg: u32,
    pub palette: Vec<(char, u32)>,
    pub rows: Vec<&'static str>,
}

pub fn brand_art() -> &'static HashMap<&'static str, Art> {
    static M: OnceLock<HashMap<&'static str, Art>> = OnceLock::new();
    M.get_or_init(|| {
        ART_DATA
            .lines()
            .filter_map(|l| {
                let mut p = l.splitn(4, '|');
                let (slug, bg, pal, rows) = (p.next()?, p.next()?, p.next()?, p.next()?);
                let palette = pal
                    .split(',')
                    .filter_map(|e| Some((e.chars().next()?, u32::from_str_radix(&e[1..], 16).ok()?)))
                    .collect();
                Some((slug, Art { bg: u32::from_str_radix(bg, 16).unwrap_or(0), palette, rows: rows.split(',').collect() }))
            })
            .collect()
    })
}

/// "arrow-down-sharp" -> "Arrow down sharp", as the picker names pixel icons.
pub fn px_name(id: &str) -> String {
    let s = id.replace('-', " ");
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

pub fn px_names() -> &'static Vec<&'static str> {
    static V: OnceLock<Vec<&'static str>> = OnceLock::new();
    V.get_or_init(|| {
        let mut v: Vec<&'static str> = px_icons().keys().copied().collect();
        v.sort_unstable();
        v
    })
}

/// The 35 built-in icons of the picker (8x8 glyphs) and their display names.
pub const BUILTIN: [(&str, &str); 35] = [
    ("play", "Play"), ("bolt", "Bolt"), ("note", "Music note"), ("folder", "Folder"), ("mic", "Microphone"),
    ("speaker", "Speaker"), ("keyboard", "Keyboard"), ("type", "Text"), ("mouse", "Mouse"), ("app", "Program"),
    ("globe", "Website"), ("monitor", "Monitor"), ("hourglass", "Timer"), ("star", "Star"), ("home", "Home"),
    ("lock", "Lock"), ("bulb", "Light"), ("power", "Power"), ("gear", "Settings"), ("check", "Check"),
    ("cross", "Cross"), ("up", "Arrow up"), ("down", "Arrow down"), ("sun", "Sun"), ("moon", "Moon"),
    ("clapper", "Scene"), ("broadcast", "Go live"), ("rewind", "Replay"), ("camera", "Camera"),
    ("playpause", "Play or pause"), ("skip", "Next track"), ("heart", "Like"), ("chat", "Chat"),
    ("flag", "Marker"), ("headset", "Headset"),
];

