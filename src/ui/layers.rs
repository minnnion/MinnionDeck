//! The background layer: everything on screen that does not move or react (title bar, rail,
//! the deck's panels and dotted ground) painted once into one picture. The views draw their
//! controls on top of it. The picture is rebuilt only when the window size, the theme, the view
//! or the pages panel changes; while one of those is animating, the views draw it live instead.
use crate::ui::theme::Theme;
use egui::{Color32, ColorImage, Context, TextureHandle, TextureOptions};

#[derive(Clone, Copy, PartialEq)]
pub struct Key {
    pub view: usize,
    pub w: u32,
    pub h: u32,
    pub dark: bool,
    /// Width of the deck's pages panel (0 when it is shut), in points.
    pub side: u32,
    pub ppp_x100: u32,
}

pub struct Layer {
    key: Option<Key>,
    tex: Option<TextureHandle>,
}

impl Layer {
    pub fn new() -> Layer {
        Layer { key: None, tex: None }
    }

    /// The texture for this key, painted again only when the key changed.
    pub fn get(&mut self, ctx: &Context, key: Key, th: &Theme, rail_w: f32, title_h: f32) -> egui::TextureId {
        if self.key != Some(key) || self.tex.is_none() {
            let img = paint(key, th, rail_w, title_h);
            match &mut self.tex {
                Some(t) => t.set(img, TextureOptions::NEAREST),
                None => self.tex = Some(ctx.load_texture("background-layer", img, TextureOptions::NEAREST)),
            }
            self.key = Some(key);
        }
        self.tex.as_ref().map(|t| t.id()).unwrap_or_default()
    }
}

struct Canvas {
    img: ColorImage,
    k: f32,
}

impl Canvas {
    /// A filled box in points, snapped to device pixels like the live drawing.
    fn fill(&mut self, x: f32, y: f32, w: f32, h: f32, c: Color32) {
        let [iw, ih] = self.img.size;
        let x0 = ((x * self.k).round().max(0.0) as usize).min(iw);
        let y0 = ((y * self.k).round().max(0.0) as usize).min(ih);
        let x1 = (((x + w) * self.k).round().max(0.0) as usize).min(iw);
        let y1 = (((y + h) * self.k).round().max(0.0) as usize).min(ih);
        for yy in y0..y1 {
            let row = &mut self.img.pixels[yy * iw..(yy + 1) * iw];
            row[x0..x1].fill(c);
        }
    }

    /// The dotted ground: a 2px dot centred in every 16px cell, counted from the area's corner.
    fn dots(&mut self, x: f32, y: f32, w: f32, h: f32, base: Color32, dot: Color32) {
        self.fill(x, y, w, h, base);
        let mut yy = 0.0;
        while yy < h {
            let mut xx = 0.0;
            while xx < w {
                for (ox, oy, dw, dh) in [(7.0, 6.0, 2.0, 1.0), (6.0, 7.0, 4.0, 2.0), (7.0, 9.0, 2.0, 1.0)] {
                    let (dx, dy) = (x + xx + ox, y + yy + oy);
                    let (cw, ch) = ((x + w - dx).min(dw), (y + h - dy).min(dh));
                    if cw > 0.0 && ch > 0.0 {
                        self.fill(dx, dy, cw, ch, dot);
                    }
                }
                xx += 16.0;
            }
            yy += 16.0;
        }
    }
}

fn paint(key: Key, th: &Theme, rail_w: f32, title_h: f32) -> ColorImage {
    let k = key.ppp_x100 as f32 / 100.0;
    let (pw, ph) = (((key.w as f32) * k).round() as usize, ((key.h as f32) * k).round() as usize);
    let mut c = Canvas { img: ColorImage::new([pw.max(1), ph.max(1)], vec![th.panel; pw.max(1) * ph.max(1)]), k };
    let (w, h) = (key.w as f32, key.h as f32);
    // title bar and rail
    c.fill(0.0, 0.0, w, title_h, th.shadow);
    c.fill(0.0, title_h, rail_w, h - title_h, th.tile);
    c.fill(rail_w - 4.0, title_h, 4.0, h - title_h, th.ink);
    // the deck: pages panel and dotted ground
    if key.view == 0 {
        let (bx, by) = (rail_w, title_h);
        let side = key.side as f32;
        if side > 0.0 {
            c.fill(bx, by, side, h - by, th.panel);
            c.fill(bx + side - 4.0, by, 4.0, h - by, th.ink);
        }
        let mx = bx + side;
        c.dots(mx, by, w - mx, h - by, th.ground, th.ground_dot);
    }
    c.img
}
