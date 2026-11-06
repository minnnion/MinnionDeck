//! A small CSS-like canvas on top of egui's painter. Everything is in logical pixels, in the
//! coordinates of the view being drawn (origin at its top left, scrolled by `org`).
use crate::ui::fonts;
use crate::ui::icons;
use crate::ui::theme::Theme;
use egui::{Color32, CursorIcon, Id, Mesh, Painter, Rect, Response, Sense, Shape, Ui, Vec2, pos2, vec2};

pub struct Cv<'a> {
    pub ui: &'a mut Ui,
    pub p: Painter,
    pub org: Vec2,
    pub clip: Rect,
    pub th: Theme,
    pub max_y: f32,
    pub id_base: &'static str,
    pub(crate) mesh: Mesh,
}

#[derive(Clone, Copy)]
pub struct BtnOpt {
    pub size: f32,
    pub weight: u16,
    pub bw: f32,
    pub shadow: f32,
    pub bg: Color32,
    pub bg_hover: Color32,
    pub fg: Color32,
    pub border: Color32,
    pub dashed: bool,
    pub disabled: bool,
}

impl<'a> Cv<'a> {
    pub fn new(ui: &'a mut Ui, clip: Rect, org: Vec2, th: Theme, id_base: &'static str) -> Self {
        let p = ui.painter().with_clip_rect(clip);
        Cv { ui, p, org, clip, th, max_y: 0.0, id_base, mesh: Mesh::default() }
    }

    pub fn abs(&self, x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(pos2(x, y) + self.org, vec2(w, h))
    }

    fn track(&mut self, y: f32, h: f32) {
        self.max_y = self.max_y.max(y + h);
    }

    // ---- shapes -------------------------------------------------------------------------
    pub fn fill(&mut self, x: f32, y: f32, w: f32, h: f32, col: Color32) {
        self.track(y, h);
        if col.a() == 0 || w <= 0.0 || h <= 0.0 {
            return;
        }
        let r = snap(self.abs(x, y, w, h));
        self.mesh.add_colored_rect(r, col);
    }

    /// Sends the batched rectangles to the painter (called before anything that must draw on top).
    pub fn flush(&mut self) {
        if !self.mesh.is_empty() {
            let m = std::mem::take(&mut self.mesh);
            self.p.add(Shape::mesh(m));
        }
    }

    /// A `border: bw solid col` ring drawn inside the box.
    pub fn border(&mut self, x: f32, y: f32, w: f32, h: f32, bw: f32, col: Color32) {
        self.fill(x, y, w, bw, col);
        self.fill(x, y + h - bw, w, bw, col);
        self.fill(x, y + bw, bw, h - 2.0 * bw, col);
        self.fill(x + w - bw, y + bw, bw, h - 2.0 * bw, col);
    }

    /// A box with a solid border and a fill (the fill covers the whole border box).
    pub fn boxed(&mut self, x: f32, y: f32, w: f32, h: f32, bw: f32, border: Color32, fill: Color32) {
        self.fill(x, y, w, h, fill);
        self.border(x, y, w, h, bw, border);
    }

    /// `border-style: dashed` as Chrome draws it: dashes 2x the width, nominal gaps 1x the width,
    /// the gap stretched (SelectBestDashGap) so both ends of a side are dashes.
    pub fn dashed(&mut self, x: f32, y: f32, w: f32, h: f32, bw: f32, col: Color32) {
        let (dash, gap0) = if bw >= 3.0 { (bw * 2.0, bw) } else { (bw * 3.0, bw * 2.0) };
        let side = |cv: &mut Self, sx: f32, sy: f32, len: f32, horizontal: bool| {
            let min_n = ((len + gap0) / (dash + gap0)).floor().max(1.0);
            let max_n = min_n + 1.0;
            let min_gap = if min_n > 1.0 { (len - min_n * dash) / (min_n - 1.0) } else { 0.0 };
            let max_gap = (len - max_n * dash) / (max_n - 1.0);
            let (n, gap) = if max_gap <= 0.0 || (min_gap - gap0).abs() < (max_gap - gap0).abs() { (min_n, min_gap) } else { (max_n, max_gap) };
            for i in 0..n as i32 {
                let o = i as f32 * (dash + gap);
                if horizontal {
                    cv.fill(sx + o, sy, dash.min(len - o), bw, col);
                } else {
                    cv.fill(sx, sy + o, bw, dash.min(len - o), col);
                }
            }
        };
        side(self, x, y, w, true);
        side(self, x, y + h - bw, w, true);
        side(self, x, y + bw, h - 2.0 * bw, false);
        side(self, x + w - bw, y + bw, h - 2.0 * bw, false);
    }

    /// `box-shadow: off off 0 col`.
    pub fn shadow(&mut self, x: f32, y: f32, w: f32, h: f32, off: f32, col: Color32) {
        self.fill(x + off, y + off, w, h, col);
    }

    /// The page's dotted ground: a 2px dot every 16px.
    /// The page's dotted ground: a 2px dot every 16px. One 16 x 16 tile, repeated by the GPU,
    /// instead of three rectangles per dot (thousands a frame on a big window).
    pub fn dots(&mut self, x: f32, y: f32, w: f32, h: f32, base: Color32, dot: Color32) {
        self.track(y, h);
        let ctx = self.ui.ctx().clone();
        let id = crate::ui::images::dot_tile(&ctx, base, dot);
        self.flush();
        let r = snap(self.abs(x, y, w, h));
        let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(r.width() / 16.0, r.height() / 16.0));
        self.p.image(id, r, uv, Color32::WHITE);
    }

    // ---- text ---------------------------------------------------------------------------
    pub fn measure(&self, size: f32, weight: u16, s: &str) -> f32 {
        self.p
            .layout_no_wrap(s.to_string(), fonts::font(size, weight), Color32::WHITE)
            .size()
            .x
    }

    /// Text on a line box: `top` is the line box's top, `lh` its height (CSS line-height).
    pub fn text(&mut self, x: f32, top: f32, lh: f32, size: f32, weight: u16, col: Color32, s: &str) -> f32 {
        self.track(top, lh);
        if s.is_empty() {
            return 0.0;
        }
        let g = self.p.layout_no_wrap(s.to_string(), fonts::font(size, weight), col);
        let w = g.size().x;
        let b = g.rows.first().and_then(|r| r.glyphs.first()).map(|gl| gl.pos.y).unwrap_or(size * 0.8);
        let (asc, desc, upm) = fonts::vmetrics();
        let a = (asc * size / upm).round();
        let d = (desc * size / upm).round();
        let base = (top + ((lh - (a + d)) / 2.0).floor() + a).round();
        self.flush();
        let o = self.org;
        self.p.galley(pos2(x + o.x, base - b + o.y), g, col);
        w
    }

    /// Text with `letter-spacing` (px), one glyph at a time.
    pub fn text_spaced(&mut self, x: f32, top: f32, lh: f32, size: f32, weight: u16, col: Color32, s: &str, ls: f32) -> f32 {
        let mut cx = x;
        for ch in s.chars() {
            let t = ch.to_string();
            let w = self.text(cx, top, lh, size, weight, col, &t);
            cx += w + ls;
        }
        cx - x
    }

    pub fn text_c(&mut self, x: f32, w: f32, top: f32, lh: f32, size: f32, weight: u16, col: Color32, s: &str) -> f32 {
        let tw = self.measure(size, weight, s);
        self.text(x + (w - tw) / 2.0, top, lh, size, weight, col, s)
    }

    pub fn text_r(&mut self, right: f32, top: f32, lh: f32, size: f32, weight: u16, col: Color32, s: &str) -> f32 {
        let tw = self.measure(size, weight, s);
        self.text(right - tw, top, lh, size, weight, col, s)
    }

    /// Single line, cut with an ellipsis when it is wider than `max`.
    pub fn text_fit(&mut self, x: f32, top: f32, lh: f32, size: f32, weight: u16, col: Color32, s: &str, max: f32) {
        if self.measure(size, weight, s) <= max {
            self.text(x, top, lh, size, weight, col, s);
            return;
        }
        let mut t: String = s.to_string();
        while !t.is_empty() && self.measure(size, weight, &format!("{t}…")) > max {
            t.pop();
        }
        self.text(x, top, lh, size, weight, col, &format!("{t}…"));
    }

    /// Word-wrapped paragraph; returns its height.
    pub fn para(&mut self, x: f32, top: f32, max_w: f32, lh: f32, size: f32, weight: u16, col: Color32, s: &str) -> f32 {
        let mut line = String::new();
        let mut y = top;
        for word in s.split_whitespace() {
            let cand = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if !line.is_empty() && self.measure(size, weight, &cand) > max_w {
                self.text(x, y, lh, size, weight, col, &line);
                y += lh;
                line = word.to_string();
            } else {
                line = cand;
            }
        }
        if !line.is_empty() {
            self.text(x, y, lh, size, weight, col, &line);
            y += lh;
        }
        y - top
    }

    pub fn wrap_lines(&self, max_w: f32, size: f32, weight: u16, s: &str) -> usize {
        let mut line = String::new();
        let mut n = 0;
        for word in s.split_whitespace() {
            let cand = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if !line.is_empty() && self.measure(size, weight, &cand) > max_w {
                n += 1;
                line = word.to_string();
            } else {
                line = cand;
            }
        }
        if !line.is_empty() { n + 1 } else { n }
    }

    // ---- pixel art ----------------------------------------------------------------------
    /// Draws `rows` (bit patterns, top bit = left) scaled to `size`, the way a browser draws
    /// `shape-rendering: crispEdges`: a device pixel is on when its centre lies inside a set cell.
    pub fn bitmap(&mut self, x: f32, y: f32, size: f32, grid: usize, rows: &[u32], col: Color32) {
        let k = grid as f32 / size;
        let (px0, px1) = (x.floor() as i32, (x + size).ceil() as i32);
        let (py0, py1) = (y.floor() as i32, (y + size).ceil() as i32);
        for py in py0..py1 {
            let cy = (py as f32 + 0.5 - y) * k;
            if cy < 0.0 || cy >= grid as f32 {
                continue;
            }
            let row = rows[cy as usize];
            let mut run: Option<i32> = None;
            for px in px0..=px1 {
                let cx = (px as f32 + 0.5 - x) * k;
                let on = px < px1 && cx >= 0.0 && cx < grid as f32 && (row >> (grid - 1 - cx as usize)) & 1 == 1;
                match (on, run) {
                    (true, None) => run = Some(px),
                    (false, Some(s)) => {
                        self.fill(s as f32, py as f32, (px - s) as f32, 1.0, col);
                        run = None;
                    }
                    _ => {}
                }
            }
        }
    }

    pub fn px_icon(&mut self, name: &str, x: f32, y: f32, size: f32, col: Color32) {
        self.px_icon_raw(name, x, y, size, col);
    }

    /// A pixel icon as pixels, whatever the icon style (the Pixel icons tab).
    pub fn px_icon_raw(&mut self, name: &str, x: f32, y: f32, size: f32, col: Color32) {
        if let Some(rows) = icons::px_icons().get(name) {
            self.bitmap(x, y, size, 24, rows, col);
        }
    }

    /// 8x8 glyph (rail icons, theme icons, tile glyphs), else the pixel icon of that name.
    pub fn glyph(&mut self, name: &str, x: f32, y: f32, size: f32, col: Color32) {
        if let Some(rows) = icons::glyphs().get(name) {
            let bits: Vec<u32> = rows
                .iter()
                .map(|r| r.chars().fold(0u32, |a, c| a << 1 | (c == '#') as u32))
                .collect();
            self.bitmap(x, y, size, 8, &bits, col);
        } else if let Some(rows) = icons::px_icons().get(name) {
            // a name the Minniondeck app has as a glyph but this set doesn't ("lightbulb"): the pixel icon
            self.bitmap(x, y, size, 24, rows, col);
        }
    }

    /// Any icon id: "px:name" (24 grid), "si:slug" (app logo; colour art when it has some) or an
    /// 8x8 glyph name. `col` is the one-colour ink.
    pub fn icon(&mut self, id: &str, x: f32, y: f32, size: f32, col: Color32) {
        if let Some(n) = id.strip_prefix("png:") {
            self.image(n, x, y, size, size);
        } else if let Some(n) = id.strip_prefix("px:") {
            self.px_icon_raw(n, x, y, size, col);
        } else if let Some(slug) = id.strip_prefix("si:") {
            if let Some(a) = icons::brand_art().get(slug) {
                let rgb = |h: u32| Color32::from_rgb((h >> 16) as u8, (h >> 8) as u8, h as u8);
                let k = size / 16.0;
                for (j, row) in a.rows.iter().enumerate() {
                    for (i, ch) in row.chars().enumerate() {
                        if let Some((_, c)) = a.palette.iter().find(|(p, _)| *p == ch) {
                            let (x0, x1) = ((x + i as f32 * k).round(), (x + (i + 1) as f32 * k).round());
                            let (y0, y1) = ((y + j as f32 * k).round(), (y + (j + 1) as f32 * k).round());
                            self.fill(x0, y0, x1 - x0, y1 - y0, rgb(*c));
                        }
                    }
                }
            } else if let Some(&i) = icons::si_by_slug().get(slug) {
                let rows = icons::si_icons()[i].rows;
                self.bitmap(x, y, size, 24, &rows, col);
            }
        } else {
            self.glyph(id, x, y, size, col);
        }
    }

    /// The tile colour an app logo sits on (its colour art background, else its brand colour).
    pub fn si_bg(&self, id: &str) -> Option<Color32> {
        let slug = id.strip_prefix("si:")?;
        let rgb = |h: u32| Color32::from_rgb((h >> 16) as u8, (h >> 8) as u8, h as u8);
        if let Some(a) = icons::brand_art().get(slug) {
            return Some(rgb(a.bg));
        }
        icons::si_by_slug().get(slug).map(|&i| rgb(icons::si_icons()[i].colour))
    }

    /// The `.pxsel-caret`: 12 x 9, rows of 12, 8 and 4 wide, 3px each.
    pub fn caret(&mut self, x: f32, y: f32, col: Color32) {
        self.fill(x, y, 12.0, 3.0, col);
        self.fill(x + 2.0, y + 3.0, 8.0, 3.0, col);
        self.fill(x + 4.0, y + 6.0, 4.0, 3.0, col);
    }

    // ---- interaction ----------------------------------------------------------------------
    pub fn hit(&mut self, id: &str, x: f32, y: f32, w: f32, h: f32) -> Response {
        let r = self.abs(x, y, w, h).intersect(self.clip);
        let id = Id::new((self.id_base, id));
        let resp = self.ui.interact(r, id, Sense::click_and_drag());
        if resp.hovered() {
            self.ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        resp
    }

    /// `.btn`: bordered box, hard shadow, pressed = shifted into the page. Returns clicked.
    pub fn button(&mut self, id: &str, x: f32, y: f32, w: f32, h: f32, label: &str, o: BtnOpt) -> bool {
        let resp = self.hit(id, x, y, w, h);
        let hot = resp.hovered() && !o.disabled;
        let down = hot && resp.is_pointer_button_down_on();
        let (dx, dy, sh) = if down { (o.shadow, o.shadow, 0.0) } else { (0.0, 0.0, o.shadow) };
        if sh > 0.0 {
            self.shadow(x, y, w, h, sh, self.th.shadow);
        }
        let bg = if hot { o.bg_hover } else { o.bg };
        self.fill(x + dx, y + dy, w, h, bg);
        if o.dashed {
            self.dashed(x + dx, y + dy, w, h, o.bw, o.border);
        } else {
            self.border(x + dx, y + dy, w, h, o.bw, o.border);
        }
        let lh = (o.size * 1.4 * 10.0).round() / 10.0;
        let col = if o.disabled { with_alpha(o.fg, 0.5) } else { o.fg };
        self.text_c(x + dx, w, y + dy + (h - lh) / 2.0, lh, o.size, o.weight, col, label);
        resp.clicked() && !o.disabled
    }

    pub fn opt_btn(&self, size: f32) -> BtnOpt {
        BtnOpt {
            size,
            weight: 600,
            bw: 4.0,
            shadow: 4.0,
            bg: self.th.tile,
            bg_hover: self.th.hover,
            fg: self.th.ink,
            border: self.th.ink,
            dashed: false,
            disabled: false,
        }
    }
}

pub fn snap(r: Rect) -> Rect {
    Rect::from_min_max(
        pos2(r.min.x.round(), r.min.y.round()),
        pos2(r.max.x.round(), r.max.y.round()),
    )
}

pub fn with_alpha(c: Color32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (a * 255.0) as u8)
}

pub fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

impl Drop for Cv<'_> {
    fn drop(&mut self) {
        self.flush();
    }
}

// ---- shared controls ----------------------------------------------------------------------
impl<'a> Cv<'a> {
    /// `.switch` (60 x 40): returns true when clicked.
    pub fn switch(&mut self, id: &str, x: f32, y: f32, on: bool) -> bool {
        let th = self.th;
        let resp = self.hit(id, x, y, 60.0, 40.0);
        let bg = if on { th.mint } else { th.tile };
        self.fill(x, y, 60.0, 40.0, bg);
        self.border(x, y, 60.0, 40.0, 4.0, th.ink);
        let ctx = self.ui.ctx().clone();
        let v = crate::ui::widgets::steps(ctx.animate_bool_with_time(Id::new((self.id_base, id, "sw")), on, 0.12), 3.0);
        let kx = x + 4.0 + 3.0 + v * 26.0;
        self.fill(kx, y + 4.0 + 6.0, 20.0, 20.0, if v >= 0.5 { th.on_accent } else { th.ink });
        resp.clicked()
    }

    /// `.pxsel-btn`: a select box with a caret. Returns the response (for opening a menu).
    pub fn select_box(&mut self, id: &str, x: f32, y: f32, w: f32, h: f32, text: &str, size: f32, bw: f32, pad_x: f32, open: bool) -> Response {
        let th = self.th;
        let resp = self.hit(id, x, y, w, h);
        if open {
            self.shadow(x, y, w, h, 4.0, th.shadow);
        }
        self.fill(x, y, w, h, if resp.hovered() { th.hover } else { th.tile });
        self.border(x, y, w, h, bw, th.ink);
        let lh = (size * 1.4 * 10.0).round() / 10.0;
        let tx = x + bw + pad_x;
        self.text_fit(tx, y + (h - lh) / 2.0, lh, size, 400, th.ink, text, w - 2.0 * (bw + pad_x) - 12.0 - 10.0);
        self.caret(x + w - bw - pad_x - 12.0, y + (h - 9.0) / 2.0, th.ink);
        resp
    }

    /// A text field (`input`): drawn only, the content is whatever the caller passes.
    pub fn input_box(&mut self, x: f32, y: f32, w: f32, h: f32, text: &str, size: f32, bw: f32, pad_x: f32) {
        let th = self.th;
        self.fill(x, y, w, h, th.tile);
        self.border(x, y, w, h, bw, th.ink);
        let lh = (size * 1.4 * 10.0).round() / 10.0;
        self.text_fit(x + bw + pad_x, y + (h - lh) / 2.0, lh, size, 400, th.ink, text, w - 2.0 * (bw + pad_x));
    }

    /// `.chip`: small bordered label. Returns its width.
    pub fn chip(&mut self, x: f32, y: f32, label: &str, size: f32, bg: Color32, fg: Color32) -> f32 {
        let th = self.th;
        let w = self.measure(size, 600, label) + 8.0 + 6.0;
        let lh = (size * 1.3 * 10.0).round() / 10.0;
        let h = lh + 6.0;
        self.fill(x, y, w, h, bg);
        self.border(x, y, w, h, 3.0, th.ink);
        self.text(x + 3.0 + 4.0, y + 3.0, lh, size, 600, fg, label);
        w
    }
}

impl<'a> Cv<'a> {
    /// A texture into the box, showing the `uv` part of it.
    pub fn tex(&mut self, id: egui::TextureId, x: f32, y: f32, w: f32, h: f32, uv: Rect) {
        self.flush();
        let r = self.abs(x, y, w, h);
        self.p.image(id, r, uv, Color32::WHITE);
    }

    /// A PNG from `images` stretched into the box, nearest-neighbour (`image-rendering: pixelated`).
    pub fn image(&mut self, name: &str, x: f32, y: f32, w: f32, h: f32) {
        let ctx = self.ui.ctx().clone();
        if let Some(id) = crate::ui::images::texture(&ctx, name) {
            self.flush();
            let r = self.abs(x, y, w, h);
            self.p.image(id, r, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        }
    }
}
