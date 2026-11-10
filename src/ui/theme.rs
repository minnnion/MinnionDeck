//! The CSS variables of 01-base.css, light and dark.
use egui::Color32;

const fn c(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

#[derive(Clone, Copy)]
// The design's whole palette: a few colours aren't drawn anywhere yet
#[allow(dead_code)]
pub struct Theme {
    pub dark: bool,
    pub ground: Color32,
    pub ground_dot: Color32,
    pub ink: Color32,
    pub slate: Color32,
    pub panel: Color32,
    pub tile: Color32,
    pub hover: Color32,
    pub shadow: Color32,
    pub rule: Color32,
    pub focus: Color32,
    pub coral_tint: Color32,
    pub on_accent: Color32,
    pub eye: Color32,
    pub mint: Color32,
    pub mint_dark: Color32,
    pub sky: Color32,
    pub sky_dark: Color32,
    pub lilac: Color32,
    pub lilac_dark: Color32,
    pub peach: Color32,
    pub peach_dark: Color32,
    pub coral: Color32,
    pub coral_dark: Color32,
    pub sun: Color32,
    pub white: Color32,
}

/// "#rrggbb" (or "#rgb") as a colour.
pub fn hex(s: &str) -> Option<Color32> {
    let h = s.strip_prefix('#')?;
    let full = match h.len() {
        3 => h.chars().flat_map(|ch| [ch, ch]).collect::<String>(),
        6 => h.to_string(),
        8 => h[..6].to_string(),
        _ => return None,
    };
    u32::from_str_radix(&full, 16).ok().map(c)
}

impl Theme {
    fn base() -> Theme {
        Theme {
            dark: false,
            ground: c(0xdfe8f1),
            ground_dot: c(0xc6d3e2),
            ink: c(0x1e2433),
            slate: c(0x56627d),
            panel: c(0xf7fafc),
            tile: c(0xffffff),
            hover: c(0xeef3f8),
            shadow: c(0x1e2433),
            rule: c(0xc6d3e2),
            focus: c(0x4c97d6),
            coral_tint: c(0xfff1f1),
            on_accent: c(0x1e2433),
            eye: c(0x1e2433),
            mint: c(0x5fd0a8),
            mint_dark: c(0x3aa585),
            sky: c(0x7cc4ff),
            sky_dark: c(0x4c97d6),
            lilac: c(0xb79cff),
            lilac_dark: c(0x8a6fe0),
            peach: c(0xffb38a),
            peach_dark: c(0xe08a5c),
            coral: c(0xff5a5f),
            coral_dark: c(0xc93c41),
            sun: c(0xffc93c),
            white: c(0xffffff),
        }
    }
    pub fn light() -> Theme {
        Theme::base()
    }
    pub fn dark() -> Theme {
        Theme {
            dark: true,
            ground: c(0x131826),
            ground_dot: c(0x222a3e),
            ink: c(0xe4eaf6),
            slate: c(0xa4b0c8),
            panel: c(0x1c2232),
            tile: c(0x262e43),
            hover: c(0x313a53),
            shadow: c(0x04060b),
            rule: c(0x3a4560),
            focus: c(0x7cc4ff),
            coral_tint: c(0x3a2129),
            ..Theme::base()
        }
    }
    /// A named accent (profile colours, button colours).
    pub fn accent(&self, name: &str) -> Color32 {
        match name {
            "mint" => self.mint,
            "sky" => self.sky,
            "lilac" => self.lilac,
            "peach" => self.peach,
            "coral" => self.coral,
            "sun" => self.sun,
            _ => hex(name).unwrap_or(self.tile),
        }
    }

    /// Text and pictures on a fill: dark on light colours, light on dark ones (a button can
    /// have any colour from the Minniondeck app, like its designs' #2d3142).
    pub fn ink_on(&self, name: &str) -> Color32 {
        match name {
            "tile" | "" => self.ink,
            n => match hex(n) {
                Some(col) if (0.299 * col.r() as f32 + 0.587 * col.g() as f32 + 0.114 * col.b() as f32) < 140.0 => c(0xe4eaf6),
                _ => self.on_accent,
            },
        }
    }
}
