//! The Deck: the pages panel (closable, slides out) and the button grid. What a button can do is
//! in its right-click menu.
use crate::ui::anim::{self, Xf};
use crate::app::App;
use crate::backend::model::Tile;
use crate::ui::draw::{Cv, lerp, with_alpha};
use egui::Color32;

/// What a button face needs to be drawn, in the grid, on the phone preview or in the editor.
pub struct Face<'a> {
    /// The look's picture and GIF, if it has any
    pub media: Option<&'a crate::backend::model::Media>,
    pub colour: &'a str,
    pub icon: &'a str,
    pub label: &'a str,
    pub dashed: bool,
    /// 0..1 while the pointer rests on it (grows 4%).
    pub hover: f32,
    /// Held down: pushed into its shadow.
    pub down: bool,
    /// A press animation and the page-switch pop (extra scale).
    pub xf: Option<Xf>,
    pub pop: f32,
}

/// The grid a picture is drawn on, so it can be scaled by whole pixels and stay sharp.
fn icon_grid(id: &str) -> f32 {
    if id.starts_with("png:") {
        16.0
    } else if id.starts_with("px:") {
        24.0
    } else if let Some(s) = id.strip_prefix("si:") {
        if crate::ui::icons::brand_art().contains_key(s) { 16.0 } else { 24.0 }
    } else {
        8.0
    }
}

/// A button: colour, border and hard shadow, the picture in the middle, the name small at the
/// bottom (Deckboard's layout: icon centred at about half the button, label under it).
pub fn tile_face(cv: &mut Cv, x: f32, y: f32, w: f32, h: f32, f: &Face) {
    let th = cv.th;
    let xf = f.xf.unwrap_or_else(Xf::id);
    let grow = (1.0 + 0.04 * f.hover) * f.pop;
    let (sw, sh) = (w * xf.sx * grow, h * xf.sy * grow);
    let push = if f.down || xf.shadow_off { 4.0 } else { 0.0 };
    let (tx, ty) = (x + (w - sw) / 2.0 + xf.dx + push, y + (h - sh) / 2.0 + xf.dy + push);
    let accent = f.colour != "tile";
    let mut bg = th.accent(f.colour);
    if xf.hue > 0.0 {
        bg = anim::hue_shift(bg, xf.hue);
    }
    if xf.flash > 0.0 {
        bg = lerp(bg, Color32::WHITE, xf.flash * 0.5);
    } else if xf.flash < 0.0 {
        bg = lerp(bg, th.shadow, -xf.flash);
    }
    if f.hover > 0.0 {
        bg = lerp(bg, Color32::WHITE, 0.06 * f.hover);
    }
    let fg = if accent { th.ink_on(f.colour) } else { th.ink };
    if push == 0.0 {
        let off = 4.0 + 2.0 * f.hover;
        cv.shadow(tx, ty, sw, sh, off, th.shadow);
    }
    cv.fill(tx, ty, sw, sh, bg);
    // a picture or a background GIF fills the inside of the border; a GIF in the icon's place
    // takes the icon's spot below
    let ctx = cv.ui.ctx().clone();
    let now = ctx.input(|i| i.time);
    let media = f.media.filter(|m| !m.is_empty());
    let mut backed = false;
    let mut gif_icon: Option<(egui::TextureId, [usize; 2])> = None;
    if let Some(m) = media {
        let (ix, iy, iw, ih) = (tx + 4.0, ty + 4.0, sw - 8.0, sh - 8.0);
        if !m.pic.is_empty() {
            if let Some((id, size)) = crate::ui::media_tex::picture(&ctx, &m.pic) {
                if m.contain {
                    let (bx, by, bw, bh) = crate::ui::media_tex::contain_box(size, iw, ih);
                    cv.tex(id, ix + bx, iy + by, bw, bh, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)));
                } else {
                    cv.tex(id, ix, iy, iw, ih, crate::ui::media_tex::cover_uv(size, iw, ih));
                }
                backed = true;
            }
        }
        if !m.gif.is_empty() {
            if let Some((id, size, next)) = crate::ui::media_tex::gif_frame(&ctx, &m.gif, now, m.speed) {
                if let Some(n) = next {
                    ctx.request_repaint_after(std::time::Duration::from_secs_f64(n.clamp(0.01, 1.0)));
                }
                if m.gif_icon {
                    gif_icon = Some((id, size));
                } else {
                    cv.tex(id, ix, iy, iw, ih, crate::ui::media_tex::cover_uv(size, iw, ih));
                    backed = true;
                }
            }
        }
    }
    if f.dashed {
        cv.dashed(tx, ty, sw, sh, 4.0, th.ink);
    } else {
        cv.border(tx, ty, sw, sh, 4.0, if xf.onair { th.coral_dark } else { th.ink });
    }
    // name: small, centred at the bottom
    let m = sw.min(sh);
    let fs = (m * 0.085).clamp(10.0, 14.0);
    let lh = (fs * 1.2).round();
    let has_label = !f.label.is_empty();
    // picture: centred, about 40% of the button, whole-pixel scale
    let grid = icon_grid(f.icon);
    let target = m * if has_label { 0.40 } else { 0.48 };
    let size = ((target / grid).floor().max(1.0) * grid).min(m - 16.0);
    let cy = ty + sh / 2.0 - if has_label { lh * 0.6 } else { 0.0 };
    match gif_icon {
        Some((id, gsize)) => {
            // the GIF in the icon's place, at the icon's height, its own shape
            let gs = (target.max(16.0)).min(m - 16.0);
            let gw = (gs * gsize[0] as f32 / gsize[1].max(1) as f32).min(sw - 16.0);
            cv.tex(id, (tx + (sw - gw) / 2.0).round(), (cy - gs / 2.0).round(), gw, gs, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)));
        }
        None => cv.icon(f.icon, (tx + (sw - size) / 2.0).round(), (cy - size / 2.0).round(), size, fg),
    }
    if has_label {
        let lw = cv.measure(fs, 600, f.label).min(sw - 20.0);
        // on a picture or a GIF the name gets a dark backing, so it stays easy to read
        let (lx, ly) = (tx + (sw - lw) / 2.0, ty + sh - 10.0 - lh);
        if backed {
            cv.fill(lx - 4.0, ly, lw + 8.0, lh, Color32::from_rgba_unmultiplied(19, 24, 38, 199));
        }
        cv.text_fit(lx, ly, lh, fs, 600, if backed { Color32::WHITE } else { fg }, f.label, sw - 20.0);
    }
    if xf.ring > 0.0 {
        cv.border(tx + 4.0, ty + 4.0, sw - 8.0, sh - 8.0, xf.ring.min((sw - 8.0) / 2.0), th.sun);
    }
    if let Some(s) = xf.scan {
        let band = (sh - 8.0) * 0.25;
        cv.fill(tx + 4.0, ty + 4.0 + (sh - 8.0 - band) * s, sw - 8.0, band, with_alpha(fg, 0.25));
    }
    if xf.fill > 0.0 {
        let fh = (sh - 8.0) * xf.fill;
        cv.fill(tx + 4.0, ty + sh - 4.0 - fh, sw - 8.0, fh, with_alpha(fg, 0.25));
    }
    if xf.onair {
        cv.fill(tx + sw - 4.0 - 44.0, ty + 6.0, 40.0, 15.0, th.coral_dark);
        cv.text_c(tx + sw - 4.0 - 44.0, 40.0, ty + 6.0, 15.0, 10.0, 700, Color32::WHITE, "ON AIR");
    }
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Width of the pages panel when it is open: 240px, 220 on a window up to 1060px wide.
pub fn side_width(window_w: f32) -> f32 {
    if window_w <= 1060.0 { 220.0 } else { 240.0 }
}

pub fn draw(app: &mut App, cv: &mut Cv, w: f32, h: f32) -> f32 {
    let th = cv.th;
    let ctx = cv.ui.ctx().clone();
    let now = cv.now();
    if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::B)) && app.editor.is_none() {
        app.side_open = !app.side_open;
    }
    // a button being dragged follows the pointer
    if let Some(d) = app.drag.as_mut() {
        if !d.fake {
            if let Some(p) = ctx.pointer_latest_pos() {
                d.pos = (p.x - cv.org.x, p.y - cv.org.y);
            }
        }
        d.page = None;
    }
    let full = side_width(w + 64.0);
    // the panel slides out to the left (0.2s, ease in and out)
    let t = ctx.animate_bool_with_time(egui::Id::new("side-open"), app.side_open, 0.2);
    let e = t * t * (3.0 - 2.0 * t);
    let side_w = (full * e).round();
    let slide = side_w - full;

    // ---- the pages panel ---------------------------------------------------------------------
    let prof = app.sample.profiles[app.sample.active].clone();
    let sx = 12.0 + slide;
    let sw = full - 4.0 - 24.0;
    if side_w > 0.0 {
        let clip = cv.clip_push(0.0, 0.0, side_w, h);
        if !app.bg_baked {
            cv.fill(0.0, 0.0, side_w, h, th.panel);
            cv.fill(side_w - 4.0, 0.0, 4.0, h, th.ink);
        }

        // profile card
        let resp = cv.hit("profcard", sx, 12.0, sw, 60.6);
        let bg = if resp.hovered() { th.hover } else { th.tile };
        if app.prof_menu {
            cv.shadow(sx, 12.0, sw, 60.6, 4.0, th.shadow);
        }
        cv.boxed(sx, 12.0, sw, 60.6, 4.0, th.ink, bg);
        cv.boxed(sx + 12.0, 12.0 + 15.3, 30.0, 30.0, 3.0, th.ink, th.accent(&prof.colour));
        cv.icon(&prof.icon, sx + 12.0 + 7.0, 12.0 + 15.3 + 7.0, 16.0, th.on_accent);
        cv.text_fit(sx + 52.0, 22.0, 22.4, 16.0, 600, th.ink, &prof.name, sw - 52.0 - 30.0);
        let sub = if prof.auto == 0 { "Switched by hand" } else { "Switches automatically" };
        cv.text_fit(sx + 52.0, 44.4, 18.2, 13.0, 400, th.slate, sub, sw - 52.0 - 30.0);
        if app.prof_menu {
            cv.caret_up(sx + sw - 4.0 - 10.0 - 12.0, 12.0 + 26.0, th.ink);
        } else {
            cv.caret(sx + sw - 4.0 - 10.0 - 12.0, 12.0 + 26.0, th.ink);
        }
        if resp.clicked() {
            app.prof_menu = !app.prof_menu;
            app.menu_at = now;
        }

        // "Pages" caption, + Page and + Folder
        cv.text_spaced(sx, 84.6, 16.8, 12.0, 600, th.slate, "PAGES", 0.72);
        let bw = (sw - 6.0) / 2.0;
        let mut link = cv.opt_btn(14.0);
        link.bw = 3.0;
        link.shadow = 0.0;
        if cv.button("addpage", sx, 107.4, bw, 34.0, "+ Page", link) {
            let n = app.sample.pages.len() + 1;
            app.sample.pages.push(crate::backend::model::PageRow::new_page(&format!("Page {n}")));
            app.fresh_page = Some((app.sample.pages.len() - 1, now));
            app.say(&format!("Page {n} added"), now);
        }
        if cv.button("addfolder", sx + bw + 6.0, 107.4, bw, 34.0, "+ Folder", link) {
            app.sample.pages.push(crate::backend::model::PageRow::new_folder("Folder"));
            app.fresh_page = Some((app.sample.pages.len() - 1, now));
        }

        // the page tree
        let mut y = 153.4;
        let mut hidden_below: Option<usize> = None;
        let rows = app.sample.pages.clone();
        for (i, row) in rows.iter().enumerate() {
            if let Some(d) = hidden_below {
                if row.depth > d {
                    continue;
                }
                hidden_below = None;
            }
            let indent = row.depth as f32 * 25.0;
            // a new row slides in from the left (slideIn: 16px, fades in)
            let mut nx = sx + indent;
            if let Some((k, t0)) = app.fresh_page {
                if k == i {
                    let p = crate::ui::widgets::steps(((now - t0) / 0.24) as f32, 3.0);
                    nx -= 16.0 * (1.0 - p);
                    if p < 1.0 {
                        ctx.request_repaint();
                    }
                }
            }
            let nw = sw - indent;
            if row.depth > 0 {
                let gx = sx + (row.depth as f32 - 1.0) * 25.0 + 14.0;
                let mut yy = y;
                while yy < y + 40.0 {
                    cv.fill(gx, yy, 3.0, 3.0, th.rule);
                    yy += 6.0;
                }
            }
            let resp = cv.hit(&format!("node{i}"), nx, y, nw, 40.0);
            // a dragged button can be dropped on another page
            let mut drop_here = false;
            if let Some(d) = app.drag.as_mut() {
                if !row.folder && i != app.sample.page {
                    if d.fake_page == Some(i) {
                        d.pos = (nx + nw * 0.62, y + 22.0);
                    }
                    drop_here = d.pos.0 >= nx && d.pos.0 < nx + nw && d.pos.1 >= y && d.pos.1 < y + 40.0;
                    if drop_here {
                        d.page = Some(i);
                    }
                }
            }
            let selected = !row.folder && i == app.sample.page || drop_here;
            if selected {
                cv.fill(nx, y, nw, 40.0, if drop_here { th.sky } else { th.mint });
                cv.border(nx, y, nw, 40.0, 3.0, th.ink);
            } else if resp.hovered() && app.drag.is_none() {
                cv.fill(nx, y, nw, 40.0, th.hover);
            }
            let fg = if selected { th.on_accent } else { th.ink };
            let tx = nx + 3.0 + 8.0;
            if row.folder {
                if row.open {
                    cv.caret(tx, y + 15.5, fg);
                } else {
                    cv.fill(tx + 1.5, y + 14.0, 3.0, 12.0, fg);
                    cv.fill(tx + 4.5, y + 16.0, 3.0, 8.0, fg);
                    cv.fill(tx + 7.5, y + 18.0, 3.0, 4.0, fg);
                }
            }
            let ix = tx + 12.0 + 8.0;
            cv.glyph(if row.folder { "folder" } else { "grid" }, ix, y + 12.0, 16.0, fg);
            let cnt = row.count.to_string();
            let cw = cv.measure(14.0, 600, &cnt);
            let right = nx + nw - 3.0 - 8.0;
            cv.text_fit(ix + 24.0, y + 9.5, 21.0, 15.0, 600, fg, &row.name, right - cw - 8.0 - (ix + 24.0));
            cv.text(right - cw, y + 10.2, 19.6, 14.0, 600, if selected { th.on_accent } else { th.slate }, &cnt);
            if resp.clicked() {
                if row.folder {
                    app.sample.pages[i].open = !app.sample.pages[i].open;
                } else if app.sample.page != i {
                    app.sample.page = i;
                    app.page_at = now;
                    app.selected_tile = None;
                }
            }
            if row.folder && !row.open {
                hidden_below = Some(row.depth);
            }
            y += 40.0;
        }
        cv.clip_pop(clip);
    }

    // ---- the main area -------------------------------------------------------------------
    let mx = side_w;
    let mw = w - side_w;
    if !app.bg_baked {
        cv.dots(mx, 0.0, mw, h, th.ground, th.ground_dot);
    }
    let px = mx + 10.0;
    let pw = mw - 20.0;

    // page bar: panel button, where you are, then the grid size and the buttons
    let tg = cv.hit("sidetoggle", px, 10.0, 32.0, 32.0);
    let hot = tg.hovered();
    cv.shadow(px, 10.0, 32.0, 32.0, if hot { 4.0 } else { 3.0 }, th.shadow);
    cv.boxed(px, 10.0, 32.0, 32.0, 3.0, th.ink, if hot { th.hover } else { th.tile });
    cv.px_icon(if app.side_open { "chevron-left" } else { "chevron-right" }, px + 4.0, 14.0, 24.0, th.ink);
    if tg.clicked() {
        app.side_open = !app.side_open;
    }
    let cx0 = px + 32.0 + 10.0;
    let page_name = app.sample.pages.get(app.sample.page).map(|p| p.name.clone()).unwrap_or_default();
    let tw = cv.text(cx0, 14.8, 22.4, 16.0, 400, th.slate, &format!("{} /", prof.name));
    cv.text(cx0 + tw + cv.measure(16.0, 400, " "), 14.8, 22.4, 16.0, 700, th.ink, &page_name);

    let edit_label = if app.edit_layout { "Done" } else { "Edit layout" };
    let phones_label = "Show on phones";
    let bwid = |cv: &Cv, label: &str| cv.measure(14.0, 600, label) + 20.0 + 8.0;
    let w_phones = bwid(cv, phones_label);
    let w_edit = bwid(cv, edit_label);
    let grid_label = format!("{} × {} grid", app.sample.cols, app.sample.rows);
    let w_sel = cv.measure(14.0, 400, &grid_label) + 3.0 * 2.0 + 16.0 + 10.0 + 12.0;
    let right = px + pw;
    let mut sm = cv.opt_btn(14.0);
    sm.bw = 4.0;
    if cv.button("phones", right - w_phones, 10.0, w_phones, 32.0, phones_label, sm) {
        app.pending_view = Some(crate::app::View::Phones);
    }
    if cv.button("edit", right - w_phones - 8.0 - w_edit, 10.0, w_edit, 32.0, edit_label, sm) {
        app.edit_layout = !app.edit_layout;
    }
    let sel_x = right - w_phones - 8.0 - w_edit - 8.0 - w_sel;
    let resp = cv.hit("gridsel", sel_x, 10.0, w_sel, 32.0);
    let bg = if resp.hovered() { th.hover } else { th.tile };
    if app.grid_menu {
        cv.shadow(sel_x, 10.0, w_sel, 32.0, 4.0, th.shadow);
    }
    cv.boxed(sel_x, 10.0, w_sel, 32.0, 3.0, th.ink, bg);
    cv.text(sel_x + 11.0, 10.0 + 6.2, 19.6, 14.0, 400, th.ink, &grid_label);
    cv.caret(sel_x + w_sel - 11.0 - 12.0, 10.0 + 11.5, th.ink);
    if resp.clicked() {
        app.grid_menu = !app.grid_menu;
        app.menu_at = now;
    }

    // grid
    let (cols, rows) = (app.sample.cols, app.sample.rows);
    let gap = 6.0;
    let area_top = 10.0 + 32.0 + 10.0;
    let area_h = h - 10.0 - area_top;
    // cell size and top-left corner of a cols x rows grid in this space
    let geom = |cols: usize, rows: usize| {
        let cell_w = (pw - gap * (cols as f32 - 1.0)) / cols as f32;
        let cell_h = (area_h - gap * (rows as f32 - 1.0)) / rows as f32;
        let cell = cell_w.min(cell_h).floor();
        let gw = cols as f32 * cell + (cols as f32 - 1.0) * gap;
        let gh = rows as f32 * cell + (rows as f32 - 1.0) * gap;
        (cell, px + ((pw - gw) / 2.0).max(0.0), area_top + ((area_h - gh) / 2.0).max(0.0))
    };
    // a grid size change glides from the old grid to the new one (0.35s, eased in and out):
    // the buttons grow or shrink in place, new cells fade in, cells and buttons that no longer
    // fit fade and shrink away
    let mut old_grid: Option<(usize, usize)> = None;
    let mut ge = 1.0_f32;
    if let Some((oc, or, t0)) = app.grid_anim {
        let p = app.grid_hold.unwrap_or(((now - t0) / GRID_ANIM) as f32);
        if p >= 1.0 || (oc, or) == (cols, rows) {
            app.grid_anim = None;
        } else {
            let p = p.max(0.0);
            ge = if p < 0.5 { 4.0 * p * p * p } else { 1.0 - (-2.0 * p + 2.0).powi(3) / 2.0 };
            old_grid = Some((oc, or));
            ctx.request_repaint();
        }
    }
    let (cell, gx, gy) = {
        let (c1, x1, y1) = geom(cols, rows);
        match old_grid {
            Some((oc, or)) => {
                let (c0, x0, y0) = geom(oc, or);
                (c0 + (c1 - c0) * ge, x0 + (x1 - x0) * ge, y0 + (y1 - y0) * ge)
            }
            None => (c1, x1, y1),
        }
    };

    let mut occupied = vec![vec![false; cols]; rows];
    let tiles: Vec<(usize, Tile)> =
        app.sample.tiles.iter().cloned().enumerate().filter(|(_, t)| t.page == app.sample.page && t.col + t.w <= cols && t.row + t.h <= rows).collect();
    for (_, t) in &tiles {
        for r in t.row..t.row + t.h {
            for c in t.col..t.col + t.w {
                occupied[r][c] = true;
            }
        }
    }
    let step = cell + gap;
    let mut any_anim = false;
    // ---- drag: which cell the dragged button's middle is over, and who makes room ----------
    if let Some(mut d) = app.drag.clone() {
        let src = app.sample.tiles.get(d.tile).filter(|t| t.page == app.sample.page).map(|t| (t.col, t.row));
        if let Some(src) = src {
            if let Some((c, r)) = d.fake_cell {
                d.grab = (cell * 0.4, cell * 0.4);
                d.pos = (gx + c as f32 * step + cell * 0.55, gy + r as f32 * step + cell * 0.5);
            }
            let (dw, dh) = app.sample.tiles.get(d.tile).map(|t| (t.w, t.h)).unwrap_or((1, 1));
            d.size = (dw, dh);
            // the cell under the middle of the dragged button's top-left cell
            let (mx_, my_) = (d.pos.0 - d.grab.0 + cell / 2.0, d.pos.1 - d.grab.1 + cell / 2.0);
            let (c, r) = (((mx_ - gx + gap / 2.0) / step).floor(), ((my_ - gy + gap / 2.0) / step).floor());
            d.over = if d.page.is_none() && c >= 0.0 && r >= 0.0 && c as usize + dw <= cols && r as usize + dh <= rows { Some((c as usize, r as usize)) } else { None };
            // buttons in the way: none = it fits; one 1x1 under a 1x1 = they swap; else blocked
            let hits: Vec<usize> = d
                .over
                .map(|(c, r)| tiles.iter().filter(|(j, t)| *j != d.tile && overlaps(t, c, r, dw, dh)).map(|(j, _)| *j).collect())
                .unwrap_or_default();
            let over_tile = match hits.as_slice() {
                [j] if (dw, dh) == (1, 1) && tiles.iter().any(|(k, t)| k == j && t.w == 1 && t.h == 1) => Some(*j),
                _ => None,
            };
            d.blocked = !hits.is_empty() && over_tile.is_none();
            if over_tile != d.over_tile && !d.fake {
                // the button under the pointer slides to the empty spot; the one before slides back
                let srcf = (src.0 as f32, src.1 as f32);
                if let Some(j) = d.over_tile {
                    let from = slid(app, j, srcf, now);
                    slide_from(app, j, (from.0, from.1), now);
                }
                if let Some(j) = over_tile {
                    let t = &app.sample.tiles[j];
                    let from = slid(app, j, (t.col as f32, t.row as f32), now);
                    slide_from(app, j, (from.0, from.1), now);
                }
            }
            d.over_tile = over_tile;
            d.src = src;
            app.drag = Some(d);
        } else {
            app.drag = None;
        }
    }
    // ---- resize: the corner handle (Edit layout) follows the pointer by whole cells ---------
    if let Some(mut rz) = app.resize.clone() {
        if let Some(t) = app.sample.tiles.get(rz.tile).filter(|t| t.page == app.sample.page).cloned() {
            let (hx, hy) = (gx + t.col as f32 * step, gy + t.row as f32 * step);
            let (w, h) = match rz.fake {
                Some((w, h)) => (w, h),
                None => match ctx.pointer_latest_pos() {
                    Some(p) => {
                        let (px_, py_) = (p.x - cv.org.x, p.y - cv.org.y);
                        (((px_ - hx + gap) / step).round().max(1.0) as usize, ((py_ - hy + gap) / step).round().max(1.0) as usize)
                    }
                    None => (rz.w, rz.h),
                },
            };
            // only four sizes: 1 × 1, 2 × 1, 1 × 2, 2 × 2
            rz.w = w.clamp(1, MAX_SPAN.min(cols - t.col));
            rz.h = h.clamp(1, MAX_SPAN.min(rows - t.row));
            rz.ok = !tiles.iter().any(|(j, o)| *j != rz.tile && overlaps(o, t.col, t.row, rz.w, rz.h));
            app.resize = Some(rz);
        } else {
            app.resize = None;
        }
    }
    let dragging = app.drag.is_some() || app.resize.is_some();
    for r in 0..rows {
        for c in 0..cols {
            if occupied[r][c] {
                continue;
            }
            let (x, y) = (gx + c as f32 * (cell + gap), gy + r as f32 * (cell + gap));
            let resp = cv.hit(&format!("slot{r}_{c}"), x, y, cell, cell);
            let hv = ctx.animate_bool_with_time(egui::Id::new(("slot-hover", r, c)), resp.hovered() && !dragging, 0.15);
            // a cell the old grid did not have fades in
            let sa = match old_grid {
                Some((oc, or)) if c >= oc || r >= or => ge,
                _ => 1.0,
            };
            if hv > 0.0 {
                cv.fill(x, y, cell, cell, with_alpha(th.hover, hv));
            }
            cv.dashed(x, y, cell, cell, 3.0, with_alpha(if hv > 0.5 { th.ink } else if app.edit_layout { th.slate } else { th.rule }, sa));
            // the plus grows when you point at it
            let ps = 16.0 + 8.0 * hv;
            let pc = with_alpha(if hv > 0.5 { th.ink } else { th.slate }, sa);
            let (pcx, pcy) = (x + cell / 2.0, y + cell / 2.0);
            cv.fill((pcx - ps / 2.0).round(), (pcy - 1.5).round(), ps.round(), 3.0, pc);
            cv.fill((pcx - 1.5).round(), (pcy - ps / 2.0).round(), 3.0, ps.round(), pc);
            if resp.clicked() {
                app.open_editor(None, Some((c, r)));
            }
            if resp.secondary_clicked() {
                if let Some(p) = resp.interact_pointer_pos() {
                    app.menu = Some(CtxMenu { tile: None, cell: (c, r), x: p.x - cv.org.x, y: p.y - cv.org.y, at: now, sub: false });
                }
            }
        }
    }
    // where the dragged button came from (an empty spot) and where it would land
    let span = |n: usize| n as f32 * cell + (n as f32 - 1.0) * gap;
    // grid change: cells and buttons the new grid has no room for fade and shrink away
    if let Some((oc, or)) = old_grid {
        let out = 1.0 - ge;
        for r in 0..or {
            for c in 0..oc {
                if c >= cols || r >= rows {
                    cv.dashed(gx + c as f32 * step, gy + r as f32 * step, cell, cell, 3.0, with_alpha(th.rule, out));
                }
            }
        }
        let fits = |t: &Tile, c: usize, r: usize| t.col + t.w <= c && t.row + t.h <= r;
        for t in app.sample.tiles.iter().filter(|t| t.page == app.sample.page && fits(t, oc, or) && !fits(t, cols, rows)) {
            let (label, icon, colour) = t.shown();
            let face = Face { media: Some(t.media_now()), colour, icon, label, dashed: app.edit_layout, hover: 0.0, down: false, xf: None, pop: out.max(0.01) };
            tile_face(cv, gx + t.col as f32 * step, gy + t.row as f32 * step, span(t.w), span(t.h), &face);
        }
    }
    if let Some(d) = &app.drag {
        let (sx_, sy_) = (gx + d.src.0 as f32 * step, gy + d.src.1 as f32 * step);
        if d.over_tile.is_none() {
            cv.dashed(sx_, sy_, span(d.size.0), span(d.size.1), 3.0, th.slate);
        }
        if let Some((c, r)) = d.over {
            let (ox, oy) = (gx + c as f32 * step, gy + r as f32 * step);
            let (ow, oh) = (span(d.size.0), span(d.size.1));
            cv.fill(ox, oy, ow, oh, if d.blocked { with_alpha(th.coral, 0.25) } else { th.hover });
            cv.dashed(ox, oy, ow, oh, 3.0, if d.blocked { th.coral } else { th.ink });
        }
    }
    let mut resize_resp: Option<egui::Response> = None;
    let mut resize_at = (0.0, 0.0);
    let mut drag_resp: Option<egui::Response> = None;
    for (k, (i, t)) in tiles.iter().enumerate() {
        let i = *i;
        let (hx, hy) = (gx + t.col as f32 * step, gy + t.row as f32 * step);
        // drawn where it is sliding to: the dragged-over button shows in the dragged one's spot
        let disp = match &app.drag {
            Some(d) if d.over_tile == Some(i) => d.src,
            _ => (t.col, t.row),
        };
        let (fc, fr, moving) = slid(app, i, (disp.0 as f32, disp.1 as f32), now);
        any_anim |= moving;
        let (x, y) = (gx + fc * step, gy + fr * step);
        let tw_ = t.w as f32 * cell + (t.w as f32 - 1.0) * gap;
        let th_ = t.h as f32 * cell + (t.h as f32 - 1.0) * gap;
        let resp = cv.hit(&format!("tile{i}"), hx, hy, tw_, th_);
        // buttons move only in Edit layout, so a press can't drag one by accident
        if resp.drag_started() && app.edit_layout && app.drag.is_none() && app.editor.is_none() && app.menu.is_none() {
            let origin = ctx.input(|inp| inp.pointer.press_origin()).or(resp.interact_pointer_pos());
            if let Some(o) = origin {
                let p = ctx.pointer_latest_pos().unwrap_or(o);
                app.drag = Some(Drag {
                    tile: i,
                    grab: (o.x - cv.org.x - hx, o.y - cv.org.y - hy),
                    pos: (p.x - cv.org.x, p.y - cv.org.y),
                    src: (t.col, t.row),
                    over: None,
                    over_tile: None,
                    size: (t.w, t.h),
                    blocked: false,
                    page: None,
                    at: now,
                    fake: false,
                    fake_cell: None,
                    fake_page: None,
                });
                app.selected_tile = Some(i);
                app.prof_menu = false;
                app.grid_menu = false;
            }
        }
        // the resize handle in the bottom-right corner (Edit layout); on top of the button's own hit
        if app.edit_layout && app.drag.is_none() {
            let rh = cv.hit(&format!("rs{i}"), hx + tw_ - 28.0, hy + th_ - 28.0, 28.0, 28.0);
            if rh.hovered() || app.resize.as_ref().map(|r| r.tile == i).unwrap_or(false) {
                ctx.set_cursor_icon(egui::CursorIcon::ResizeNwSe);
            }
            if rh.drag_started() && app.resize.is_none() && app.editor.is_none() && app.menu.is_none() {
                app.resize = Some(Resize { tile: i, w: t.w, h: t.h, ok: true, fake: None });
                app.selected_tile = Some(i);
            }
            if app.resize.as_ref().map(|r| r.tile == i).unwrap_or(false) {
                resize_resp = Some(rh);
            }
        }
        if app.resize.as_ref().map(|r| r.tile == i).unwrap_or(false) {
            // drawn last, at the size it is being stretched to
            resize_at = (hx, hy);
            continue;
        }
        if app.drag.as_ref().map(|d| d.tile == i).unwrap_or(false) {
            // drawn last, under the pointer
            drag_resp = Some(resp);
            continue;
        }
        let hover = ctx.animate_bool_with_time(egui::Id::new(("tile-hover", i)), resp.hovered() && !app.edit_layout && !dragging, 0.2);
        let down = resp.hovered() && resp.is_pointer_button_down_on() && !app.edit_layout && !dragging;
        if down && t.action.starts_with("plug:") {
            app.plug.held = Some(i);
        }
        // a knob turns with the wheel (wheel up turns it clockwise)
        if resp.hovered() && !app.edit_layout && crate::app_plugins::is_knob(&t.action) {
            let dy = ctx.input(|inp| inp.smooth_scroll_delta.y);
            if dy != 0.0 {
                app.plug.turn(i, if dy > 0.0 { 1 } else { -1 }, now);
            }
        }
        let xf = app.tile_anim.filter(|(j, _)| *j == i).and_then(|(_, t0)| anim::eval(t.anim, now - t0));
        // switching page: the buttons pop in one after another (0.88 -> 1, 25ms apart)
        let pt = ((now - app.page_at) as f32 - k as f32 * 0.025) / 0.25;
        let mut pop = if pt < 1.0 { 0.88 + 0.12 * ease(pt.max(0.0)) } else { 1.0 };
        if let Some((j, t0)) = app.fresh_tile {
            if j == i {
                let p = ((now - t0) as f32 / 0.3).clamp(0.0, 1.0);
                // popIn: 0.5 -> 1.1 -> 1
                pop *= if p < 0.6 { 0.5 + 0.6 * ease(p / 0.6) } else { 1.1 - 0.1 * ease((p - 0.6) / 0.4) };
                any_anim |= p < 1.0;
            }
        }
        // grid change: a button the old grid had no room for grows in
        if let Some((oc, or)) = old_grid {
            if t.col + t.w > oc || t.row + t.h > or {
                pop *= ge.max(0.01);
            }
        }
        any_anim |= pt < 1.0 || xf.is_some() || (hover > 0.0 && hover < 1.0);
        let (label, icon, colour) = t.shown();
        // a look change flips the button over (0.22s), the new look shows from the middle on
        let mut fx = 1.0;
        if let Some((j, t0)) = app.flip {
            if j == i {
                let p = ((now - t0) as f32 / 0.22).clamp(0.0, 1.0);
                fx = (1.0 - 2.0 * p).abs().max(0.08);
                any_anim |= p < 1.0;
            }
        }
        let face = Face { media: Some(t.media_now()), colour, icon, label, dashed: app.edit_layout, hover, down, xf, pop };
        let fw_ = tw_ * fx;
        tile_face(cv, x + (tw_ - fw_) / 2.0, y, fw_, th_, &face);
        if app.selected_tile == Some(i) {
            cv.border(x - 2.0 - 6.0, y - 2.0 - 6.0, tw_ + 4.0 + 12.0, th_ + 4.0 + 12.0, 4.0, th.coral_dark);
        }
        // the press ripple: a square that grows from the middle and fades
        for (j, t0) in &app.ripples {
            if *j != i {
                continue;
            }
            let p = ((now - t0) as f32 / 0.45).clamp(0.0, 1.0);
            let q = ease(p);
            let (rw, rh) = ((tw_ - 8.0) * (0.2 + 0.8 * q), (th_ - 8.0) * (0.2 + 0.8 * q));
            let a = 0.45 * (1.0 - p);
            cv.border(x + (tw_ - rw) / 2.0, y + (th_ - rh) / 2.0, rw, rh, 4.0, with_alpha(Color32::WHITE, a));
            any_anim = true;
        }
        if app.edit_layout {
            cv.fill(x + tw_ - 28.0, y + th_ - 28.0, 28.0, 28.0, th.coral);
            cv.fill(x + tw_ - 28.0, y + th_ - 28.0, 28.0, 4.0, th.ink);
            cv.fill(x + tw_ - 28.0, y + th_ - 28.0, 4.0, 28.0, th.ink);
        }
        if resp.clicked() {
            app.selected_tile = if app.selected_tile == Some(i) { None } else { Some(i) };
            if !app.edit_layout {
                app.ripples.push((i, now));
                app.pending_press = Some(i);
                // a plugin switches its button's look itself (once plugins run)
                if t.looks() > 1 && !crate::app_plugins::owns_states(&t.action) {
                    let n = t.looks();
                    app.sample.tiles[i].cur = (t.cur + 1) % n;
                    app.flip = Some((i, now));
                }
                if t.anim != 0 {
                    app.tile_anim = Some((i, now));
                }
            }
        }
        if resp.double_clicked() {
            app.open_editor(Some(i), None);
        }
        if resp.secondary_clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                app.selected_tile = Some(i);
                app.menu = Some(CtxMenu { tile: Some(i), cell: (t.col, t.row), x: p.x - cv.org.x, y: p.y - cv.org.y, at: now, sub: false });
            }
        }
    }
    // the dragged button floats under the pointer, lifted; over a page it shrinks
    if let Some(d) = app.drag.clone() {
        if let Some(t) = app.sample.tiles.get(d.tile) {
            let lift = ease(((now - d.at) / 0.12) as f32);
            any_anim |= lift < 1.0;
            let s = ctx.animate_value_with_time(egui::Id::new("drag-shrink"), if d.page.is_some() { 0.5 } else { 1.0 }, 0.12);
            let (label, icon, colour) = t.shown();
            let face = Face { media: Some(t.media_now()), colour, icon, label, dashed: app.edit_layout, hover: lift, down: false, xf: None, pop: 1.0 + 0.04 * lift };
            // over a page it shrinks to one small square
            let (fw, fh) = if d.page.is_some() { (cell * s, cell * s) } else { (span(t.w), span(t.h)) };
            tile_face(cv, d.pos.0 - d.grab.0 * s, d.pos.1 - d.grab.1 * s, fw, fh, &face);
        }
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        // (--drop: the screenshot helper lets go after a few frames)
        let fake_drop = d.fake && app.frames >= 3 && std::env::args().any(|a| a == "--drop");
        let released = drag_resp.map(|r| r.drag_stopped()).unwrap_or(false) || (!d.fake && !ctx.input(|i| i.pointer.primary_down())) || fake_drop;
        let cancel = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if released || cancel {
            app.drag = None;
            drop_drag(app, d, cancel, (gx, gy, step), now);
            any_anim = true;
        }
    }
    // the button being resized: the cells it will cover, the button itself growing to them (the
    // live preview), its size, and the handle
    if let Some(rz) = app.resize.clone() {
        if let Some(t) = app.sample.tiles.get(rz.tile).cloned() {
            let (hx, hy) = resize_at;
            let (aw, ah) = (span(rz.w), span(rz.h));
            let edge = if rz.ok { th.ink } else { th.coral };
            cv.fill(hx, hy, aw, ah, if rz.ok { th.hover } else { with_alpha(th.coral, 0.25) });
            cv.dashed(hx, hy, aw, ah, 3.0, edge);
            // blocked: the button keeps its own size so the ones in the way stay visible
            let (gw_, gh_) = if rz.ok { (aw, ah) } else { (span(t.w), span(t.h)) };
            let pw_ = ctx.animate_value_with_time(egui::Id::new(("rs-w", rz.tile)), gw_, 0.1);
            let ph_ = ctx.animate_value_with_time(egui::Id::new(("rs-h", rz.tile)), gh_, 0.1);
            any_anim |= (pw_ - gw_).abs() > 0.5 || (ph_ - gh_).abs() > 0.5;
            let (label, icon, colour) = t.shown();
            let face = Face { media: Some(t.media_now()), colour, icon, label, dashed: true, hover: 0.0, down: false, xf: None, pop: 1.0 };
            tile_face(cv, hx, hy, pw_, ph_, &face);
            if rz.ok {
                cv.border(hx - 8.0, hy - 8.0, pw_ + 16.0, ph_ + 16.0, 4.0, th.coral_dark);
            } else {
                cv.border(hx - 8.0, hy - 8.0, aw + 16.0, ah + 16.0, 4.0, th.coral);
            }
            // the size, in a small tag at the top left
            let tag = if rz.ok { format!("{} × {}", rz.w, rz.h) } else { format!("{} × {}  No room", rz.w, rz.h) };
            let tw = cv.measure(13.0, 700, &tag) + 16.0;
            cv.boxed(hx + 8.0, hy + 8.0, tw, 24.0, 3.0, th.ink, if rz.ok { th.sun } else { th.coral });
            cv.text(hx + 16.0, hy + 10.5, 19.0, 13.0, 700, th.on_accent, &tag);
            let (cx_, cy_) = (hx + pw_ - 28.0, hy + ph_ - 28.0);
            cv.fill(cx_, cy_, 28.0, 28.0, th.coral);
            cv.fill(cx_, cy_, 28.0, 4.0, th.ink);
            cv.fill(cx_, cy_, 4.0, 28.0, th.ink);
            ctx.set_cursor_icon(egui::CursorIcon::ResizeNwSe);
        }
        let fake_drop = rz.fake.is_some() && app.frames >= 3 && std::env::args().any(|a| a == "--drop");
        let released = resize_resp.map(|r| r.drag_stopped()).unwrap_or(false) || (rz.fake.is_none() && !ctx.input(|i| i.pointer.primary_down())) || fake_drop;
        let cancel = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if released || cancel {
            app.resize = None;
            if let Some(t) = app.sample.tiles.get_mut(rz.tile) {
                if cancel {
                } else if !rz.ok {
                    app.say("No room there", now);
                } else if (t.w, t.h) != (rz.w, rz.h) {
                    t.w = rz.w;
                    t.h = rz.h;
                    app.say(&format!("Button is now {} × {}", rz.w, rz.h), now);
                }
            }
            any_anim = true;
        }
    }
    app.slides.retain(|s| now - s.3 < SLIDE);
    app.ripples.retain(|(_, t0)| now - t0 < 0.45);
    if let Some((_, t0)) = app.tile_anim {
        if now - t0 > anim::DURATION {
            app.tile_anim = None;
        }
    }
    if any_anim || !app.ripples.is_empty() || app.tile_anim.is_some() {
        ctx.request_repaint();
    }

    // ---- dropdown menus (drawn last, on top) ------------------------------------------------
    let reveal = crate::ui::widgets::steps(((now - app.menu_at) / 0.16) as f32, 4.0);
    if reveal < 1.0 {
        ctx.request_repaint();
    }
    if app.prof_menu && side_w > 0.0 {
        let n = app.sample.profiles.len();
        let mh = 8.0 + 44.0 * n as f32 + 8.0;
        let clip = cv.clip_push(sx, 12.0 + 60.6, sw + 8.0, (mh + 12.0) * reveal);
        cv.shadow(sx, 12.0 + 60.6 + 6.0, sw, mh, 6.0, th.shadow);
        cv.boxed(sx, 12.0 + 60.6 + 6.0, sw, mh, 4.0, th.ink, th.panel);
        for (i, p) in app.sample.profiles.clone().iter().enumerate() {
            let y = 12.0 + 60.6 + 6.0 + 8.0 + 44.0 * i as f32;
            let resp = cv.hit(&format!("pm{i}"), sx + 8.0, y, sw - 16.0, 44.0);
            if resp.hovered() {
                cv.fill(sx + 8.0, y, sw - 16.0, 44.0, th.hover);
            }
            cv.boxed(sx + 14.0, y + 7.0, 30.0, 30.0, 3.0, th.ink, th.accent(&p.colour));
            cv.icon(&p.icon, sx + 14.0 + 7.0, y + 14.0, 16.0, th.on_accent);
            cv.text_fit(sx + 54.0, y + 11.5, 21.0, 15.0, 600, th.ink, &p.name, sw - 54.0 - 40.0);
            if i == app.sample.active {
                cv.glyph("check", sx + sw - 16.0 - 16.0, y + 14.0, 16.0, th.ink);
            }
            if resp.clicked() {
                if app.sample.active != i {
                    app.page_at = now;
                    app.say(&format!("Switched to {}", p.name), now);
                }
                app.sample.active = i;
                app.prof_menu = false;
            }
        }
        cv.clip_pop(clip);
    }
    context_menu(app, cv, w, h, now);
    keyboard(app, cv, now);
    if app.grid_menu {
        let opts = [(5, 3), (4, 3), (3, 3)];
        let mw2 = 150.0;
        let mx2 = (sel_x + w_sel - mw2).max(mx + 10.0);
        let my = 10.0 + 32.0 + 4.0;
        let mh = 8.0 + 8.0 + 38.0 * opts.len() as f32;
        let clip = cv.clip_push(mx2, my, mw2 + 8.0, (mh + 8.0) * reveal);
        cv.shadow(mx2, my, mw2, mh, 6.0, th.shadow);
        cv.boxed(mx2, my, mw2, mh, 4.0, th.ink, th.panel);
        for (i, (c, r)) in opts.iter().enumerate() {
            let y = my + 8.0 + 38.0 * i as f32;
            let resp = cv.hit(&format!("gm{i}"), mx2 + 8.0, y, mw2 - 16.0, 38.0);
            let selected = *c == app.sample.cols && *r == app.sample.rows;
            if selected {
                cv.fill(mx2 + 8.0, y, mw2 - 16.0, 38.0, th.mint);
            } else if resp.hovered() {
                cv.fill(mx2 + 8.0, y, mw2 - 16.0, 38.0, th.hover);
            }
            let fg = if selected { th.on_accent } else { th.ink };
            cv.text(mx2 + 18.0, y + 9.0, 21.0, 15.0, if selected { 700 } else { 400 }, fg, &format!("{c} × {r} grid"));
            if resp.clicked() {
                if (*c, *r) != (app.sample.cols, app.sample.rows) {
                    app.grid_anim = Some((app.sample.cols, app.sample.rows, now));
                }
                app.sample.cols = *c;
                app.sample.rows = *r;
                app.grid_menu = false;
            }
        }
        cv.clip_pop(clip);
    }
    h
}

/// A button being dragged to a new spot (or onto a page in the pages panel).
#[derive(Clone)]
pub struct Drag {
    pub tile: usize,
    /// Where on the button it was picked up, and the pointer now (deck coordinates).
    pub grab: (f32, f32),
    pub pos: (f32, f32),
    /// The cell it came from, the cell it would land on, and the button there (they swap).
    pub src: (usize, usize),
    pub over: Option<(usize, usize)>,
    pub over_tile: Option<usize>,
    /// Its size in cells, and whether other buttons are in the way where it would land.
    pub size: (usize, usize),
    pub blocked: bool,
    /// A page row under the pointer: dropping moves the button to that page.
    pub page: Option<usize>,
    pub at: f64,
    /// Screenshot helper (--drag / --drag-page): the pointer is held at a cell or page row.
    pub fake: bool,
    pub fake_cell: Option<(usize, usize)>,
    pub fake_page: Option<usize>,
}

/// A button being stretched by its corner handle in Edit layout (size in cells).
#[derive(Clone)]
pub struct Resize {
    pub tile: usize,
    pub w: usize,
    pub h: usize,
    /// No other button is in the way.
    pub ok: bool,
    /// Screenshot helper (--resize T W H): the size is held there.
    pub fake: Option<(usize, usize)>,
}

/// How long a grid size change glides.
const GRID_ANIM: f64 = 0.35;

/// A button is at most 2 cells wide and 2 tall.
pub const MAX_SPAN: usize = 2;

/// Whether button `t` covers any cell of the area at (c, r), w x h cells.
fn overlaps(t: &Tile, c: usize, r: usize, w: usize, h: usize) -> bool {
    t.col < c + w && c < t.col + t.w && t.row < r + h && r < t.row + t.h
}

/// How long a button takes to glide to its new spot.
const SLIDE: f64 = 0.18;

/// Where button `i` shows now (in cells), gliding towards `to`; and whether it still moves.
fn slid(app: &App, i: usize, to: (f32, f32), now: f64) -> (f32, f32, bool) {
    if let Some(&(_, fx, fy, t0)) = app.slides.iter().find(|s| s.0 == i) {
        let p = ((now - t0) / SLIDE) as f32;
        if p < 1.0 {
            let e = ease(p);
            return (fx + (to.0 - fx) * e, fy + (to.1 - fy) * e, true);
        }
    }
    (to.0, to.1, false)
}

/// Starts button `i` gliding from `from` (in cells) to wherever it belongs.
fn slide_from(app: &mut App, i: usize, from: (f32, f32), now: f64) {
    app.slides.retain(|s| s.0 != i);
    app.slides.push((i, from.0, from.1, now));
}

/// Lets go of a dragged button: onto a page, onto a free cell, swapped with the button there,
/// or (outside the grid, or Escape) back where it was.
fn drop_drag(app: &mut App, d: Drag, cancel: bool, (gx, gy, step): (f32, f32, f32), now: f64) {
    let i = d.tile;
    if i >= app.sample.tiles.len() {
        return;
    }
    let float = ((d.pos.0 - d.grab.0 - gx) / step, (d.pos.1 - d.grab.1 - gy) / step);
    let srcf = (d.src.0 as f32, d.src.1 as f32);
    if !cancel {
        if let Some(page) = d.page {
            move_to_page(app, i, page, now);
            return;
        }
        if let Some((c, r)) = d.over.filter(|_| !d.blocked) {
            if (c, r) != d.src {
                if let Some(j) = d.over_tile {
                    app.sample.tiles[j].col = d.src.0;
                    app.sample.tiles[j].row = d.src.1;
                }
                app.sample.tiles[i].col = c;
                app.sample.tiles[i].row = r;
            }
            slide_from(app, i, float, now);
            return;
        }
    }
    // back to where it was; a button that made room goes back too
    slide_from(app, i, float, now);
    if let Some(j) = d.over_tile {
        let from = slid(app, j, srcf, now);
        slide_from(app, j, (from.0, from.1), now);
    }
}

/// Takes button `i` off this page and puts it in the first free cell of `page`.
fn move_to_page(app: &mut App, i: usize, page: usize, now: f64) {
    let Some((c, r)) = free_cell(app, page) else {
        app.say("No free spot on that page", now);
        return;
    };
    let from = app.sample.tiles[i].page;
    let tl = &mut app.sample.tiles[i];
    tl.page = page;
    tl.col = c;
    tl.row = r;
    tl.w = 1;
    tl.h = 1;
    page_count_add(app, from, -1);
    page_count_add(app, page, 1);
    app.selected_tile = None;
    app.slides.retain(|s| s.0 != i);
    let name = app.sample.pages[page].name.clone();
    app.say(&format!("Moved to {name}"), now);
}

/// The first cell on the page no button covers.
pub fn free_cell(app: &App, page: usize) -> Option<(usize, usize)> {
    let (cols, rows) = (app.sample.cols, app.sample.rows);
    let mut occ = vec![vec![false; cols]; rows];
    for t in app.sample.tiles.iter().filter(|t| t.page == page) {
        for r in t.row..(t.row + t.h).min(rows) {
            for c in t.col..(t.col + t.w).min(cols) {
                occ[r][c] = true;
            }
        }
    }
    for r in 0..rows {
        for c in 0..cols {
            if !occ[r][c] {
                return Some((c, r));
            }
        }
    }
    None
}

/// The right-click menu: on a button (Edit, Duplicate, Copy, Paste, Move to page, Delete) or on
/// an empty slot (New button here, Paste here). Opens where you clicked.
#[derive(Clone)]
pub struct CtxMenu {
    pub tile: Option<usize>,
    pub cell: (usize, usize),
    pub x: f32,
    pub y: f32,
    pub at: f64,
    pub sub: bool,
}

enum Cmd {
    Edit,
    Duplicate,
    Copy,
    Paste,
    Move,
    Delete,
    New,
}

fn page_count_add(app: &mut App, page: usize, d: i32) {
    if let Some(p) = app.sample.pages.get_mut(page) {
        p.count = (p.count as i32 + d).max(0) as usize;
    }
}

/// Puts a copy of `t` on `page`, at `cell` when given and free, else at the first free cell.
fn place_copy(app: &mut App, mut t: Tile, page: usize, cell: Option<(usize, usize)>, now: f64) -> bool {
    let spot = cell.or_else(|| free_cell(app, page));
    let Some((c, r)) = spot else {
        app.say("No free spot on that page", now);
        return false;
    };
    t.page = page;
    t.col = c;
    t.row = r;
    t.w = 1;
    t.h = 1;
    t.cur = 0;
    // a new button that keeps the look and settings of the one it copies
    if !t.id.is_empty() {
        t.src = std::mem::take(&mut t.id);
    }
    app.sample.tiles.push(t);
    page_count_add(app, page, 1);
    if page == app.sample.page {
        app.fresh_tile = Some((app.sample.tiles.len() - 1, now));
    }
    true
}

fn run_cmd(app: &mut App, cmd: Cmd, m: &CtxMenu, target_page: Option<usize>, now: f64) {
    let tile = m.tile.and_then(|i| app.sample.tiles.get(i).cloned());
    match cmd {
        Cmd::Edit => app.open_editor(m.tile, None),
        Cmd::New => app.open_editor(None, Some(m.cell)),
        Cmd::Duplicate => {
            if let Some(t) = tile {
                if place_copy(app, t, app.sample.page, None, now) {
                    app.say("Button duplicated", now);
                }
            }
        }
        Cmd::Copy => {
            if let Some(t) = tile {
                app.say(&format!("Copied {}", if t.label.is_empty() { "the button" } else { &t.label }), now);
                app.clipboard = Some(t);
            }
        }
        Cmd::Paste => {
            if let Some(t) = app.clipboard.clone() {
                let cell = if m.tile.is_none() { Some(m.cell) } else { None };
                if place_copy(app, t, app.sample.page, cell, now) {
                    app.say("Pasted", now);
                }
            }
        }
        Cmd::Move => {
            if let (Some(i), Some(_), Some(page)) = (m.tile, tile, target_page) {
                move_to_page(app, i, page, now);
            }
        }
        Cmd::Delete => {
            if let Some(i) = m.tile {
                if i < app.sample.tiles.len() {
                    let page = app.sample.tiles[i].page;
                    app.sample.tiles.remove(i);
                    app.slides.clear();
                    page_count_add(app, page, -1);
                    app.selected_tile = None;
                    app.say("Button deleted", now);
                }
            }
        }
    }
}

fn context_menu(app: &mut App, cv: &mut Cv, w: f32, h: f32, now: f64) {
    let Some(m) = app.menu.clone() else { return };
    let th = cv.th;
    let ctx = cv.ui.ctx().clone();
    let has_clip = app.clipboard.is_some();
    // (label, icon, command, enabled, danger)
    let items: Vec<(&str, &str, Option<Cmd>, bool, bool)> = if m.tile.is_some() {
        vec![
            ("Edit button", "pencil", Some(Cmd::Edit), true, false),
            ("Duplicate", "copy", Some(Cmd::Duplicate), true, false),
            ("Copy", "clipboard", Some(Cmd::Copy), true, false),
            ("Paste", "clipboard-note", Some(Cmd::Paste), has_clip, false),
            ("Move to page", "move", None, app.sample.pages.iter().filter(|p| !p.folder).count() > 1, false),
            ("Delete", "trash", Some(Cmd::Delete), true, true),
        ]
    } else {
        vec![("New button here", "plus", Some(Cmd::New), true, false), ("Paste here", "clipboard-note", Some(Cmd::Paste), has_clip, false)]
    };
    let mw = 210.0;
    let ih = 36.0;
    let mh = 8.0 + items.len() as f32 * ih + if m.tile.is_some() { 10.0 } else { 0.0 } + 8.0;
    let mx = m.x.min(w - mw - 12.0).max(4.0);
    let my = m.y.min(h - mh - 12.0).max(4.0);
    // opens with a quick drop (0.14s, four steps), like the other menus
    let reveal = crate::ui::widgets::steps(((now - m.at) / 0.14) as f32, 4.0);
    if reveal < 1.0 {
        ctx.request_repaint();
    }
    let clip = cv.clip_push(mx - 2.0, my - 2.0, mw + 260.0, (mh + 12.0) * reveal + 2.0);
    cv.shadow(mx, my, mw, mh, 6.0, th.shadow);
    cv.boxed(mx, my, mw, mh, 4.0, th.ink, th.panel);
    let mut y = my + 8.0;
    let mut chosen: Option<(Cmd, Option<usize>)> = None;
    let mut over_move = false;
    let mut move_y = 0.0;
    for (k, (label, icon, cmd, enabled, danger)) in items.into_iter().enumerate() {
        if danger {
            cv.fill(mx + 10.0, y + 4.0, mw - 20.0, 2.0, th.rule);
            y += 10.0;
        }
        let r = cv.hit(&format!("cm{k}"), mx + 6.0, y, mw - 12.0, ih);
        let hot = r.hovered() && enabled;
        if hot {
            cv.fill(mx + 6.0, y, mw - 12.0, ih, if danger { th.coral_dark } else { th.hover });
        }
        let fg = if !enabled { with_alpha(th.slate, 0.6) } else if danger && hot { Color32::WHITE } else if danger { th.coral } else { th.ink };
        cv.px_icon(icon, mx + 14.0, y + 8.0, 20.0, fg);
        cv.text(mx + 44.0, y + 8.0, 20.0, 15.0, 600, fg, label);
        if cmd.is_none() {
            // Move to page: a submenu to the right
            cv.px_icon("chevron-right", mx + mw - 30.0, y + 8.0, 20.0, fg);
            if hot || m.sub {
                over_move = true;
                move_y = y;
            }
            if r.clicked() && enabled {
                if let Some(mm) = app.menu.as_mut() {
                    mm.sub = !mm.sub;
                }
            }
        } else if r.clicked() && enabled {
            chosen = cmd.map(|c| (c, None));
        }
        y += ih;
    }
    // the pages to move to
    let mut sub_rect = None;
    if over_move && m.tile.is_some() {
        if let Some(mm) = app.menu.as_mut() {
            mm.sub = true;
        }
        let pages: Vec<(usize, String)> = app.sample.pages.iter().enumerate().filter(|(i, p)| !p.folder && *i != app.sample.page).map(|(i, p)| (i, p.name.clone())).collect();
        let sw = 190.0;
        let sh = 8.0 + pages.len() as f32 * ih + 8.0;
        let sx = if mx + mw + sw + 8.0 < w { mx + mw - 6.0 } else { mx - sw + 6.0 };
        let sy = move_y - 8.0;
        cv.shadow(sx, sy, sw, sh, 6.0, th.shadow);
        cv.boxed(sx, sy, sw, sh, 4.0, th.ink, th.panel);
        for (k, (pi, name)) in pages.iter().enumerate() {
            let yy = sy + 8.0 + k as f32 * ih;
            let r = cv.hit(&format!("mvp{pi}"), sx + 6.0, yy, sw - 12.0, ih);
            if r.hovered() {
                cv.fill(sx + 6.0, yy, sw - 12.0, ih, th.hover);
            }
            cv.glyph("grid", sx + 16.0, yy + 10.0, 16.0, th.ink);
            cv.text_fit(sx + 42.0, yy + 8.0, 20.0, 15.0, 600, th.ink, name, sw - 54.0);
            if r.clicked() {
                chosen = Some((Cmd::Move, Some(*pi)));
            }
        }
        sub_rect = Some(cv.abs(sx, sy, sw, sh));
    }
    cv.clip_pop(clip);

    let menu_rect = cv.abs(mx, my, mw, mh);
    if let Some((cmd, page)) = chosen {
        app.menu = None;
        run_cmd(app, cmd, &m, page, now);
        return;
    }
    // a click anywhere else, or Escape, closes it
    let outside = ctx.input(|i| {
        i.pointer.any_pressed()
            && i.pointer.interact_pos().map(|p| !menu_rect.contains(p) && !sub_rect.map(|r| r.contains(p)).unwrap_or(false)).unwrap_or(true)
    });
    let opened_now = now - m.at < 0.05;
    if (outside && !opened_now) || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.menu = None;
    }
}

/// With a button picked: Ctrl+C copies, Ctrl+V pastes, Ctrl+D duplicates, Delete removes it,
/// Enter edits it.
fn keyboard(app: &mut App, cv: &mut Cv, now: f64) {
    if app.editor.is_some() || app.menu.is_some() || app.drag.is_some() || app.resize.is_some() {
        return;
    }
    let ctx = cv.ui.ctx().clone();
    if ctx.memory(|m| m.focused().is_some()) {
        return;
    }
    let events = ctx.input(|i| i.events.clone());
    let sel = app.selected_tile;
    let m = |tile: Option<usize>| CtxMenu { tile, cell: (0, 0), x: 0.0, y: 0.0, at: now, sub: false };
    for e in events {
        match e {
            egui::Event::Copy if sel.is_some() => run_cmd(app, Cmd::Copy, &m(sel), None, now),
            egui::Event::Paste(_) if app.clipboard.is_some() => run_cmd(app, Cmd::Paste, &m(sel), None, now),
            egui::Event::Key { key: egui::Key::D, pressed: true, modifiers, .. } if modifiers.ctrl && sel.is_some() => run_cmd(app, Cmd::Duplicate, &m(sel), None, now),
            egui::Event::Key { key: egui::Key::Delete, pressed: true, .. } if sel.is_some() => run_cmd(app, Cmd::Delete, &m(sel), None, now),
            egui::Event::Key { key: egui::Key::Enter, pressed: true, .. } if sel.is_some() => run_cmd(app, Cmd::Edit, &m(sel), None, now),
            _ => {}
        }
    }
}
