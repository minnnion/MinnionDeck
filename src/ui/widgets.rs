//! Text fields, key capture, clipping and stepped animation on top of the canvas.
use crate::ui::draw::Cv;
use egui::{Color32, Id, Painter};

/// CSS `steps(n, end)`: the value jumps at the end of each n-th of the way.
pub fn steps(p: f32, n: f32) -> f32 {
    if p >= 1.0 { 1.0 } else { (p.clamp(0.0, 1.0) * n).floor() / n }
}

impl<'a> Cv<'a> {
    /// Further drawing is clipped to the box (local coordinates) until `clip_pop`.
    pub fn clip_push(&mut self, x: f32, y: f32, w: f32, h: f32) -> Painter {
        self.flush();
        let old = self.p.clone();
        let r = self.abs(x, y, w, h).intersect(self.clip);
        self.p = old.with_clip_rect(r);
        old
    }

    pub fn clip_pop(&mut self, old: Painter) {
        self.flush();
        self.p = old;
    }

    pub fn now(&self) -> f64 {
        self.ui.input(|i| i.time)
    }

    /// An `input`: click to focus, type to edit. The caret blinks while it has focus.
    pub fn text_field(&mut self, id: &str, x: f32, y: f32, w: f32, h: f32, value: &mut String, size: f32, placeholder: &str) {
        self.field(id, x, y, w, h, value, size, placeholder, false)
    }

    /// A text field that shows a star for each character (passwords).
    #[allow(clippy::too_many_arguments)]
    pub fn password_field(&mut self, id: &str, x: f32, y: f32, w: f32, h: f32, value: &mut String, size: f32, placeholder: &str) {
        self.field(id, x, y, w, h, value, size, placeholder, true)
    }

    #[allow(clippy::too_many_arguments)]
    fn field(&mut self, id: &str, x: f32, y: f32, w: f32, h: f32, value: &mut String, size: f32, placeholder: &str, masked: bool) {
        let th = self.th;
        let fid = Id::new((self.id_base, id));
        let resp = self.hit(id, x, y, w, h);
        if resp.clicked() {
            self.ui.memory_mut(|m| m.request_focus(fid));
        }
        let focused = self.ui.memory(|m| m.has_focus(fid));
        if focused {
            let events = self.ui.input(|i| i.events.clone());
            for e in events {
                match e {
                    egui::Event::Text(t) | egui::Event::Paste(t) => value.push_str(&t.replace('\n', "")),
                    egui::Event::Key { key: egui::Key::Backspace, pressed: true, .. } => {
                        value.pop();
                    }
                    _ => {}
                }
            }
            self.ui.ctx().request_repaint();
        }
        self.fill(x, y, w, h, th.tile);
        self.border(x, y, w, h, 3.0, if focused { th.focus } else { th.ink });
        let lh = (size * 1.35 * 100.0).round() / 100.0;
        let ty = y + (h - lh) / 2.0;
        let max = w - 22.0;
        let visible = if masked { "*".repeat(value.chars().count()) } else { value.clone() };
        if value.is_empty() {
            self.text_fit(x + 11.0, ty, lh, size, 400, th.slate, placeholder, max);
        } else {
            let mut shown = visible.clone();
            while self.measure(size, 400, &shown) > max && shown.chars().count() > 1 {
                shown.remove(0);
            }
            self.text(x + 11.0, ty, lh, size, 400, th.ink, &shown);
        }
        if focused && (self.now() * 2.0) as i64 % 2 == 0 {
            let tw = if value.is_empty() { 0.0 } else { self.measure(size, 400, &visible).min(max) };
            self.fill(x + 11.0 + tw, ty + 1.0, 2.0, lh - 2.0, th.ink);
        }
    }

    /// The Keys box: click, then press the keys together. Backspace clears it.
    pub fn key_field(&mut self, id: &str, x: f32, y: f32, w: f32, h: f32, value: &mut String, size: f32, placeholder: &str) {
        let th = self.th;
        let fid = Id::new((self.id_base, id));
        let resp = self.hit(id, x, y, w, h);
        if resp.clicked() {
            self.ui.memory_mut(|m| m.request_focus(fid));
        }
        let focused = self.ui.memory(|m| m.has_focus(fid));
        if focused {
            let events = self.ui.input(|i| i.events.clone());
            for e in events {
                if let egui::Event::Key { key, pressed: true, modifiers, .. } = e {
                    if key == egui::Key::Backspace {
                        value.clear();
                        continue;
                    }
                    let mut parts: Vec<String> = vec![];
                    if modifiers.ctrl {
                        parts.push("Ctrl".into());
                    }
                    if modifiers.alt {
                        parts.push("Alt".into());
                    }
                    if modifiers.shift {
                        parts.push("Shift".into());
                    }
                    let k = format!("{key:?}");
                    parts.push(k.strip_prefix("Num").filter(|r| r.len() == 1).unwrap_or(&k).to_string());
                    *value = parts.join("+");
                }
            }
            self.ui.ctx().request_repaint();
        }
        self.fill(x, y, w, h, th.tile);
        self.border(x, y, w, h, 3.0, if focused { th.focus } else { th.ink });
        let lh = (size * 1.35 * 100.0).round() / 100.0;
        let ty = y + (h - lh) / 2.0;
        if value.is_empty() {
            let hint = if focused { "Press the keys now" } else { placeholder };
            self.text_fit(x + 11.0, ty, lh, size, 400, th.slate, hint, w - 22.0);
        } else {
            self.text_fit(x + 11.0, ty, lh, size, 600, th.ink, value, w - 22.0);
        }
    }

    /// A pointing-up caret (the open state of a section header).
    pub fn caret_up(&mut self, x: f32, y: f32, col: Color32) {
        self.fill(x + 4.0, y, 4.0, 3.0, col);
        self.fill(x + 2.0, y + 3.0, 8.0, 3.0, col);
        self.fill(x, y + 6.0, 12.0, 3.0, col);
    }
}

/// The pixel scrollbar of 07-controls.css: a 14px gutter, a thin groove, a mint thumb.
pub fn scrollbar(ui: &mut egui::Ui, th: crate::ui::theme::Theme, region: egui::Rect, content: f32, scroll: &mut f32, tag: &str) {
    let view_h = region.height();
    if content <= view_h + 0.5 {
        *scroll = 0.0;
        return;
    }
    let track = egui::Rect::from_min_size(egui::pos2(region.max.x - 14.0, region.min.y), egui::vec2(14.0, view_h));
    let mut cv = Cv::new(ui, region, region.min.to_vec2(), th, "sb");
    let x = region.width() - 14.0;
    cv.fill(x, 0.0, 14.0, view_h, th.panel);
    cv.fill(x + 6.0, 0.0, 2.0, view_h, th.ground_dot);
    let thumb_h = (view_h * view_h / content).max(30.0);
    let max_scroll = content - view_h;
    let ty = (*scroll / max_scroll) * (view_h - thumb_h);
    let resp = cv.ui.interact(track, Id::new(("sbtrack", tag)), egui::Sense::click_and_drag());
    if resp.dragged() {
        let dy = resp.drag_delta().y;
        *scroll = (*scroll + dy * max_scroll / (view_h - thumb_h)).clamp(0.0, max_scroll);
    }
    let hot = resp.hovered();
    let (fill, shade) = if resp.dragged() { (th.sun, th.sun) } else if hot { (th.sky, th.sky_dark) } else { (th.mint, th.mint_dark) };
    let (tx, tw, thh) = (x + 3.0, 8.0, thumb_h - 6.0);
    let ty = ty + 3.0;
    cv.fill(tx, ty, tw, thh, fill);
    if !resp.dragged() {
        cv.fill(tx + tw - 4.0, ty, 4.0, thh, shade);
        cv.fill(tx, ty + thh - 4.0, tw, 4.0, shade);
    }
    cv.border(tx, ty, tw, thh, 2.0, th.ink);
}
