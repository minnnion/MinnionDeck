//! Marketplace: Discover (search, categories, one card per plugin), Installed (what is on this
//! PC and whether it is up to date) and Updates (what each new version brings), plus a plugin
//! details window with its "What's new" list. The data is generated (src/market_data.rs); the
//! installed versions are a sample until the backend exists.
use crate::app::App;
use crate::ui::draw::{Cv, lerp, with_alpha};
use crate::generated::market_data::{MarketPlugin, Release};

/// The plugins on offer: the marketplace's own list when there is one (backend/market.rs).
fn market() -> &'static [MarketPlugin] {
    crate::backend::market::plugins()
}
use egui::{Color32, Rect, Sense, Ui, pos2, vec2};

const CATS: [(&str, &str); 5] = [("", "All"), ("general", "General"), ("streaming", "Streaming"), ("games", "Games"), ("work", "Work")];
/// How long a pretend install or update takes.
const BUSY: f64 = 1.4;

/// The details window.
#[derive(Clone)]
pub struct Detail {
    pub index: usize,
    pub opened: Option<f64>,
    pub closing: Option<f64>,
    pub scroll: f32,
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Compares two "1.2.3" versions.
pub fn newer(a: &str, b: &str) -> bool {
    let p = |s: &str| s.split('.').map(|x| x.parse::<u32>().unwrap_or(0)).collect::<Vec<_>>();
    p(a) > p(b)
}

fn size_text(b: u64) -> String {
    if b == 0 {
        String::new()
    } else if b >= 1_000_000 {
        format!("{:.1} MB", b as f64 / 1_000_000.0)
    } else {
        format!("{} KB", (b / 1000).max(1))
    }
}

fn cat_name(c: &str) -> &'static str {
    CATS.iter().find(|k| k.0 == c).map(|k| k.1).unwrap_or("General")
}

fn plugin_icon(id: &str) -> &'static str {
    crate::backend::catalog::plugins().iter().find(|p| p.id == id).map(|p| p.icon).unwrap_or("bolt")
}

/// The version of plugin `id` on this PC, if it is installed.
fn installed(app: &App, id: &str) -> Option<String> {
    app.market_installed.iter().find(|(k, _)| *k == id).map(|(_, v)| v.clone())
}

#[derive(Clone, Copy, PartialEq)]
enum State {
    Get,
    Busy(f32),
    Current,
    Update,
}

/// A real install or update running: which plugin, how far the download is, and what it will say.
pub struct MarketJob {
    pub index: usize,
    pub frac: std::sync::Arc<std::sync::atomic::AtomicU32>,
    pub rx: std::sync::mpsc::Receiver<Result<crate::backend::market::Installed, String>>,
    pub was: Option<String>,
}

fn state(app: &App, i: usize, now: f64) -> State {
    if let Some(j) = &app.market_job {
        if j.index == i {
            return State::Busy(f32::from_bits(j.frac.load(std::sync::atomic::Ordering::Relaxed)));
        }
    }
    if let Some((k, t0)) = app.market_busy {
        if k == i {
            return State::Busy((((now - t0) / BUSY) as f32).clamp(0.0, 1.0));
        }
    }
    if app.market_queue.contains(&i) {
        return State::Busy(0.0);
    }
    match installed(app, market()[i].id) {
        None => State::Get,
        Some(v) if newer(market()[i].version, &v) => State::Update,
        Some(_) => State::Current,
    }
}

fn updates(app: &App) -> Vec<usize> {
    (0..market().len()).filter(|&i| installed(app, market()[i].id).map(|v| newer(market()[i].version, &v)).unwrap_or(false)).collect()
}

/// Starts installing or updating plugin `i` (one at a time; the rest wait in a queue).
fn start(app: &mut App, i: usize, now: f64) {
    if app.market_busy.map(|(k, _)| k == i).unwrap_or(false) || app.market_queue.contains(&i) || app.market_job.as_ref().is_some_and(|j| j.index == i) {
        return;
    }
    if app.plug.running() {
        if app.market_job.is_some() {
            app.market_queue.push(i);
        } else {
            spawn_job(app, i);
        }
        return;
    }
    if app.market_busy.is_none() {
        app.market_busy = Some((i, now));
    } else {
        app.market_queue.push(i);
    }
}

/// Moves the pretend installs along: finishes the one running, starts the next.
/// Downloads, checks, installs and starts plugin `i` on its own thread (backend/market.rs).
fn spawn_job(app: &mut App, i: usize) {
    use std::sync::atomic::{AtomicU32, Ordering};
    let p = &market()[i];
    let frac = std::sync::Arc::new(AtomicU32::new(0f32.to_bits()));
    let (tx, rx) = std::sync::mpsc::channel();
    let (id, root, f, ctx) = (p.id.to_string(), app.lib.paths.root.clone(), frac.clone(), app.egui.clone());
    std::thread::spawn(move || {
        let r = crate::backend::market::install(&root, &id, None, |got, total| {
            f.store(((got as f32 / total.max(1) as f32) * 0.95).to_bits(), Ordering::Relaxed);
            ctx.request_repaint();
        });
        let _ = tx.send(r);
        ctx.request_repaint();
    });
    app.market_job = Some(MarketJob { index: i, frac, rx, was: installed(app, p.id) });
}

fn tick(app: &mut App, now: f64, ctx: &egui::Context) {
    if let Some(j) = &app.market_job {
        ctx.request_repaint();
        if let Ok(r) = j.rx.try_recv() {
            let (was, p) = (j.was.clone(), &market()[j.index]);
            app.market_job = None;
            match r {
                Ok(done) => match was {
                    Some(_) => app.say(&format!("{} updated to {}", p.name, done.version), now),
                    None => app.say(&format!("{} installed. Its buttons are in Edit button.", p.name), now),
                },
                Err(e) => app.say(&format!("Could not install {}: {e}", p.name), now),
            }
            if !app.market_queue.is_empty() {
                let next = app.market_queue.remove(0);
                spawn_job(app, next);
            }
        }
        return;
    }
    if let Some((i, t0)) = app.market_busy {
        ctx.request_repaint();
        if now - t0 >= BUSY {
            let p = &market()[i];
            let was = installed(app, p.id);
            app.market_installed.retain(|(k, _)| *k != p.id);
            app.market_installed.push((p.id, p.version.to_string()));
            app.market_busy = None;
            match was {
                Some(_) => app.say(&format!("{} updated to {}", p.name, p.version), now),
                None => app.say(&format!("{} installed. Its buttons are in Edit button.", p.name), now),
            }
            if !app.market_queue.is_empty() {
                let next = app.market_queue.remove(0);
                app.market_busy = Some((next, now));
            }
        }
    }
}

fn btn_primary(cv: &Cv) -> crate::ui::draw::BtnOpt {
    let th = cv.th;
    let mut o = cv.opt_btn(14.0);
    o.bg = th.mint;
    o.bg_hover = lerp(th.mint, Color32::WHITE, 0.15);
    o.fg = th.on_accent;
    o
}

fn btn_update(cv: &Cv) -> crate::ui::draw::BtnOpt {
    let th = cv.th;
    let mut o = cv.opt_btn(14.0);
    o.bg = th.sun;
    o.bg_hover = lerp(th.sun, Color32::WHITE, 0.15);
    o.fg = th.on_accent;
    o
}

/// The one button that does the next thing for plugin `i`: Install, Update, a progress bar while
/// it works, or a quiet "Installed".
fn action_button(app: &mut App, cv: &mut Cv, id: &str, i: usize, x: f32, y: f32, w: f32, h: f32, now: f64) {
    let th = cv.th;
    match state(app, i, now) {
        State::Get => {
            if cv.button(id, x, y, w, h, "Install", btn_primary(cv)) {
                start(app, i, now);
            }
        }
        State::Update => {
            if cv.button(id, x, y, w, h, "Update", btn_update(cv)) {
                start(app, i, now);
            }
        }
        State::Busy(p) => {
            cv.fill(x, y, w, h, th.panel);
            cv.fill(x + 3.0, y + 3.0, ((w - 6.0) * p).round(), h - 6.0, with_alpha(th.mint, 0.45));
            cv.border(x, y, w, h, 3.0, th.ink);
            let label = if p == 0.0 { "Waiting…".to_string() } else { format!("{}%", (p * 100.0) as u32) };
            cv.text_c(x, w, y + (h - 19.6) / 2.0, 19.6, 13.0, 700, th.ink, &label);
        }
        State::Current => {
            cv.border(x, y, w, h, 3.0, th.rule);
            let lw = cv.measure(13.0, 600, "Installed") + 22.0;
            let lx = x + (w - lw) / 2.0;
            cv.glyph("check", lx, y + (h - 16.0) / 2.0, 16.0, th.mint);
            cv.text(lx + 22.0, y + (h - 19.6) / 2.0, 19.6, 13.0, 600, th.slate, "Installed");
        }
    }
}

/// A plugin's icon on its colour, in a bordered square.
fn icon_box(cv: &mut Cv, p: &MarketPlugin, x: f32, y: f32, s: f32) {
    let th = cv.th;
    cv.boxed(x, y, s, s, 3.0, th.ink, th.accent(p.colour));
    let inner = ((s - 16.0) / 16.0).floor().max(1.0) * 16.0;
    cv.icon(plugin_icon(p.id), x + (s - inner) / 2.0, y + (s - inner) / 2.0, inner, th.on_accent);
}

/// "Official" with a check, after the author.
fn byline(cv: &mut Cv, p: &MarketPlugin, x: f32, y: f32, max: f32) {
    let th = cv.th;
    let a = format!("by {}", p.author);
    let aw = cv.measure(13.0, 400, &a).min(max);
    cv.text_fit(x, y, 19.0, 13.0, 400, th.slate, &a, max);
    if p.official && aw + 90.0 < max {
        cv.glyph("check", x + aw + 10.0, y + 2.0, 14.0, th.mint);
        cv.text(x + aw + 28.0, y, 19.0, 13.0, 600, th.mint, "Official");
    }
}

pub fn draw(app: &mut App, cv: &mut Cv, w: f32, h: f32) -> f32 {
    let th = cv.th;
    let now = cv.now();
    let ctx = cv.ui.ctx().clone();
    tick(app, now, &ctx);
    cv.fill(0.0, 0.0, w, h.max(1500.0) + 600.0, th.panel);
    let cw = w - 14.0 - 32.0;
    let x = 16.0;

    // ---- head: title, what it is, search ---------------------------------------------------
    cv.text(x, 22.0, 28.0, 20.0, 700, th.ink, "Marketplace");
    let sw = 320.0_f32.min(cw * 0.4);
    cv.para(x, 54.0, (cw - sw - 24.0).min(680.0), 21.0, 15.0, 400, th.slate, "Free plugins that add new buttons to your deck. Each one is checked before it installs.");
    let sx = x + cw - sw;
    let mut query = app.market_query.clone();
    cv.text_field("mkq", sx, 18.0, sw, 42.0, &mut query, 15.0, "");
    if query.is_empty() {
        cv.glyph("mkSearch", sx + 12.0, 18.0 + 12.0, 18.0, th.slate);
        cv.text(sx + 40.0, 18.0 + 10.5, 21.0, 15.0, 400, th.slate, "Search plugins and buttons");
    }
    if query != app.market_query {
        app.market_query = query;
        app.market_tab = 0;
    }

    // ---- tabs ------------------------------------------------------------------------------
    let ups = updates(app);
    let n_inst = app.market_installed.len();
    let ty = 96.0;
    cv.fill(x, ty + 40.0, cw, 2.0, th.rule);
    let mut tx = x;
    let tabs = [("Discover", 0usize), ("Installed", n_inst), ("Updates", ups.len())];
    for (k, (label, n)) in tabs.iter().enumerate() {
        let lw = cv.measure(15.0, 700, label);
        let badge = if *n > 0 { cv.measure(12.0, 700, &n.to_string()).max(8.0) + 12.0 } else { 0.0 };
        let tw = lw + if badge > 0.0 { badge + 8.0 } else { 0.0 } + 24.0;
        let r = cv.hit(&format!("mtab{k}"), tx, ty, tw, 42.0);
        let on = app.market_tab == k;
        if r.hovered() && !on {
            cv.fill(tx, ty, tw, 40.0, th.hover);
        }
        cv.text(tx + 12.0, ty + 9.0, 22.0, 15.0, 700, if on { th.ink } else { th.slate }, label);
        if *n > 0 {
            let (bg, fg) = if k == 2 { (th.sun, th.on_accent) } else { (th.tile, th.ink) };
            let bx = tx + 12.0 + lw + 8.0;
            cv.boxed(bx, ty + 10.0, badge, 20.0, 2.0, th.ink, bg);
            cv.text_c(bx, badge, ty + 11.5, 17.0, 12.0, 700, fg, &n.to_string());
        }
        if on {
            cv.fill(tx, ty + 38.0, tw, 4.0, th.mint);
        }
        if r.clicked() {
            app.market_tab = k;
        }
        tx += tw + 4.0;
    }
    // last checked, and Check now
    let o = cv.opt_btn(13.0);
    let wc = cv.measure(13.0, 600, "Check now") + 24.0;
    if cv.button("mcheck", x + cw - wc, ty + 2.0, wc, 32.0, "Check now", o) {
        app.market_checked = now;
        app.start_market_refresh(true);
        app.say(if ups.is_empty() { "Everything is up to date" } else { "Checked: updates are ready" }, now);
    }
    let checked = if now - app.market_checked < 60.0 { "Checked for updates just now" } else { "Checked for updates today" };
    cv.text_r(x + cw - wc - 12.0, ty + 9.0, 19.0, 13.0, 400, th.slate, checked);

    let top = ty + 42.0 + 20.0;
    let bottom = match app.market_tab {
        1 => installed_tab(app, cv, x, top, cw, now),
        2 => updates_tab(app, cv, x, top, cw, now),
        _ => discover(app, cv, x, top, cw, now, &ups),
    };
    bottom + 24.0
}

fn matches(p: &MarketPlugin, q: &str) -> bool {
    if q.trim().is_empty() {
        return true;
    }
    let q = q.trim().to_lowercase();
    p.name.to_lowercase().contains(&q) || p.summary.to_lowercase().contains(&q) || p.actions.iter().any(|a| a.to_lowercase().contains(&q))
}

#[allow(clippy::too_many_arguments)]
fn discover(app: &mut App, cv: &mut Cv, x: f32, y: f32, cw: f32, now: f64, ups: &[usize]) -> f32 {
    let th = cv.th;
    let mut y = y;
    // updates waiting: one calm banner
    if !ups.is_empty() && app.market_query.is_empty() {
        let bh = 64.0;
        cv.boxed(x, y, cw, bh, 3.0, th.ink, lerp(th.tile, th.sun, 0.12));
        cv.fill(x + 3.0, y + 3.0, 6.0, bh - 6.0, th.sun);
        cv.glyph("bolt", x + 24.0, y + 20.0, 24.0, th.sun);
        let title = if ups.len() == 1 { "1 update is ready".to_string() } else { format!("{} updates are ready", ups.len()) };
        cv.text(x + 62.0, y + 11.0, 22.0, 16.0, 700, th.ink, &title);
        let names: Vec<&str> = ups.iter().map(|&i| market()[i].name).collect();
        let sub = format!("New versions of {}.", join_names(&names));
        let wa = cv.measure(14.0, 600, "Update all") + 28.0;
        let wn = cv.measure(14.0, 600, "See what's new") + 28.0;
        cv.text_fit(x + 62.0, y + 35.0, 19.0, 13.0, 400, th.slate, &sub, cw - 62.0 - wa - wn - 40.0);
        if cv.button("mupall", x + cw - 14.0 - wa, y + 12.0, wa, 40.0, "Update all", btn_update(cv)) {
            for &i in ups {
                start(app, i, now);
            }
        }
        if cv.button("mseenew", x + cw - 14.0 - wa - 10.0 - wn, y + 12.0, wn, 40.0, "See what's new", cv.opt_btn(14.0)) {
            app.market_tab = 2;
        }
        y += bh + 20.0;
    }

    // categories that have plugins, with how many
    let list: Vec<usize> = (0..market().len()).filter(|&i| matches(&market()[i], &app.market_query)).collect();
    let mut cx = x;
    for (k, (key, label)) in CATS.iter().enumerate() {
        let n = list.iter().filter(|&&i| key.is_empty() || market()[i].category == *key).count();
        if n == 0 && !key.is_empty() {
            continue;
        }
        let text = format!("{label}  {n}");
        let bw = cv.measure(13.0, 600, &text) + 26.0;
        let r = cv.hit(&format!("mcat{k}"), cx, y, bw, 32.0);
        let on = app.market_cat == k;
        cv.fill(cx, y, bw, 32.0, if on { th.ink } else if r.hovered() { th.hover } else { th.tile });
        cv.border(cx, y, bw, 32.0, 3.0, th.ink);
        cv.text_c(cx, bw, y + 6.0, 20.0, 13.0, 600, if on { th.panel } else { th.ink }, &text);
        if r.clicked() {
            app.market_cat = k;
        }
        cx += bw + 8.0;
    }
    y += 32.0 + 18.0;

    let cat = CATS.get(app.market_cat).map(|c| c.0).unwrap_or("");
    let mut shown: Vec<usize> = list.into_iter().filter(|&i| cat.is_empty() || market()[i].category == cat).collect();
    shown.sort_by_key(|&i| market()[i].name);
    if shown.is_empty() {
        cv.dashed(x, y, cw, 120.0, 3.0, th.rule);
        cv.text_c(x, cw, y + 38.0, 22.0, 16.0, 700, th.ink, "No plugins match");
        cv.text_c(x, cw, y + 64.0, 19.0, 13.0, 400, th.slate, "Try another word, or pick All.");
        return y + 120.0;
    }
    let cols = if cw >= 900.0 { 3 } else { 2 };
    let gap = 16.0;
    let card_w = ((cw - gap * (cols as f32 - 1.0)) / cols as f32).floor();
    let card_h = 196.0;
    for (k, &i) in shown.iter().enumerate() {
        let (r, c) = (k / cols, k % cols);
        card(app, cv, i, k, x + c as f32 * (card_w + gap), y + r as f32 * (card_h + gap), card_w, card_h, now);
    }
    let rows = shown.len().div_ceil(cols);
    y + rows as f32 * card_h + (rows - 1) as f32 * gap
}

fn join_names(n: &[&str]) -> String {
    match n.len() {
        0 => String::new(),
        1 => n[0].to_string(),
        _ => format!("{} and {}", n[..n.len() - 1].join(", "), n[n.len() - 1]),
    }
}

#[allow(clippy::too_many_arguments)]
fn card(app: &mut App, cv: &mut Cv, i: usize, k: usize, x: f32, y: f32, w: f32, h: f32, now: f64) {
    let th = cv.th;
    let ctx = cv.ui.ctx().clone();
    let p = &market()[i];
    // the cards rise in one after another when the page opens
    let t = ((app.view_t - k as f32 * 0.04) / 0.3).clamp(0.0, 1.0);
    if t < 1.0 {
        ctx.request_repaint();
    }
    let y = y + 14.0 * (1.0 - ease(t));
    let resp = cv.hit(&format!("mcard{i}"), x, y, w, h);
    let hv = ctx.animate_bool_with_time(egui::Id::new(("mcard-hover", i)), resp.hovered(), 0.15);
    if hv > 0.0 && hv < 1.0 {
        ctx.request_repaint();
    }
    let y = y - 2.0 * hv;
    cv.shadow(x, y, w, h, 4.0 + 2.0 * hv, th.shadow);
    cv.boxed(x, y, w, h, 4.0, th.ink, if hv > 0.0 { lerp(th.tile, th.hover, hv * 0.5) } else { th.tile });
    let ix = x + 18.0;
    icon_box(cv, p, ix, y + 18.0, 56.0);
    let tx = ix + 56.0 + 14.0;
    let st = state(app, i, now);
    let tag_w = if st == State::Update { cv.measure(12.0, 700, p.version) + 40.0 } else { 0.0 };
    cv.text_fit(tx, y + 20.0, 24.0, 17.0, 700, th.ink, p.name, x + w - 18.0 - tag_w - tx);
    if st == State::Update {
        let tag = format!("New {}", p.version);
        let tw = cv.measure(12.0, 700, &tag) + 14.0;
        cv.boxed(x + w - 18.0 - tw, y + 22.0, tw, 22.0, 2.0, th.ink, th.sun);
        cv.text(x + w - 18.0 - tw + 7.0, y + 23.5, 17.0, 12.0, 700, th.on_accent, &tag);
    }
    byline(cv, p, tx, y + 46.0, x + w - 18.0 - tx);
    // two lines of summary at most
    let clip = cv.clip_push(ix, y + 88.0, w - 36.0, 42.0);
    cv.para(ix, y + 88.0, w - 36.0, 21.0, 14.0, 400, th.ink, p.summary);
    cv.clip_pop(clip);
    // what you get, and the button
    let by = y + h - 18.0 - 38.0;
    let bw = 112.0;
    let mut meta = format!("{} action{}", p.actions.len(), if p.actions.len() == 1 { "" } else { "s" });
    let sz = size_text(p.size);
    if !sz.is_empty() {
        meta.push_str(" · ");
        meta.push_str(&sz);
    }
    if p.profile_pages > 0 {
        meta.push_str(" · profile");
    }
    cv.text_fit(ix, by + 10.0, 19.0, 13.0, 400, th.slate, &meta, w - 36.0 - bw - 12.0);
    action_button(app, cv, &format!("mact{i}"), i, x + w - 18.0 - bw, by, bw, 38.0, now);
    if resp.clicked() {
        app.market_detail = Some(Detail { index: i, opened: None, closing: None, scroll: 0.0 });
    }
}

fn installed_tab(app: &mut App, cv: &mut Cv, x: f32, y: f32, cw: f32, now: f64) -> f32 {
    let th = cv.th;
    let mut list: Vec<usize> = (0..market().len()).filter(|&i| installed(app, market()[i].id).is_some()).collect();
    list.sort_by_key(|&i| market()[i].name);
    if list.is_empty() {
        cv.dashed(x, y, cw, 140.0, 3.0, th.rule);
        cv.text_c(x, cw, y + 44.0, 22.0, 16.0, 700, th.ink, "No plugins yet");
        cv.text_c(x, cw, y + 70.0, 19.0, 13.0, 400, th.slate, "Install one from Discover and its buttons show up in Edit button.");
        return y + 140.0;
    }
    let rh = 80.0;
    let box_h = list.len() as f32 * rh + 8.0;
    cv.boxed(x, y, cw, box_h, 4.0, th.ink, th.tile);
    for (k, &i) in list.iter().enumerate() {
        let p = &market()[i];
        let ry = y + 4.0 + k as f32 * rh;
        if k > 0 {
            cv.fill(x + 18.0, ry, cw - 36.0, 2.0, th.rule);
        }
        let r = cv.hit(&format!("mrow{i}"), x + 4.0, ry + 2.0, cw - 8.0, rh - 4.0);
        if r.hovered() {
            cv.fill(x + 4.0, ry + 2.0, cw - 8.0, rh - 4.0, with_alpha(th.hover, 0.6));
        }
        icon_box(cv, p, x + 18.0, ry + 14.0, 48.0);
        let tx = x + 18.0 + 48.0 + 14.0;
        let v = installed(app, p.id).unwrap_or_default();
        cv.text_fit(tx, ry + 15.0, 23.0, 16.0, 700, th.ink, p.name, 300.0);
        let sub = format!("Version {v} · {} action{}", p.actions.len(), if p.actions.len() == 1 { "" } else { "s" });
        cv.text_fit(tx, ry + 41.0, 19.0, 13.0, 400, th.slate, &sub, 300.0);
        // status in the middle
        let st = state(app, i, now);
        let mx = x + cw * 0.5;
        // the buttons start here; the status text stops short of them
        let btn_left = x + cw - 18.0 - 86.0 - 8.0 - 86.0 - 8.0 - 96.0 - 14.0;
        match st {
            State::Update => {
                cv.glyph("bolt", mx, ry + 24.0, 16.0, th.sun);
                cv.text(mx + 24.0, ry + 15.0, 20.0, 14.0, 700, th.ink, &format!("{} is ready", p.version));
                let title = p.releases.first().map(|r| r.title).unwrap_or("");
                cv.text_fit(mx + 24.0, ry + 38.0, 19.0, 13.0, 400, th.slate, if title.is_empty() { "A new version" } else { title }, btn_left - (mx + 24.0));
            }
            State::Current => {
                cv.glyph("check", mx, ry + 31.0, 16.0, th.mint);
                cv.text(mx + 24.0, ry + 28.0, 20.0, 14.0, 600, th.slate, "Up to date");
            }
            State::Busy(_) => {
                cv.text(mx + 24.0, ry + 28.0, 20.0, 14.0, 600, th.slate, &format!("Updating to {}…", p.version));
            }
            State::Get => {}
        }
        // buttons on the right: Update (when there is one), Details, Remove
        let right = x + cw - 18.0;
        let o = cv.opt_btn(13.0);
        let confirm = app.market_remove == Some(i);
        let mut ro = o;
        if confirm {
            ro.bg = th.coral_dark;
            ro.bg_hover = th.coral;
            ro.fg = Color32::WHITE;
        } else {
            ro.fg = th.coral;
        }
        if cv.button(&format!("mrm{i}"), right - 86.0, ry + 21.0, 86.0, 38.0, if confirm { "Sure?" } else { "Remove" }, ro) {
            if confirm {
                if app.plug.running() {
                    let (id, tx, ctx) = (p.id.to_string(), app.ran_tx.clone(), app.egui.clone());
                    std::thread::spawn(move || {
                        if let Err(e) = crate::backend::plugins::remove(&id) {
                            let _ = tx.send(Err(e));
                            ctx.request_repaint();
                        }
                    });
                } else {
                    app.market_installed.retain(|(id, _)| *id != p.id);
                }
                app.market_remove = None;
                app.say(&format!("{} removed", p.name), now);
            } else {
                app.market_remove = Some(i);
            }
        }
        if cv.button(&format!("mdet{i}"), right - 86.0 - 8.0 - 86.0, ry + 21.0, 86.0, 38.0, "Details", o) {
            app.market_detail = Some(Detail { index: i, opened: None, closing: None, scroll: 0.0 });
        }
        if matches!(st, State::Update | State::Busy(_)) {
            action_button(app, cv, &format!("mupd{i}"), i, right - 86.0 - 8.0 - 86.0 - 8.0 - 96.0, ry + 21.0, 96.0, 38.0, now);
        }
    }
    y + box_h
}

/// The releases of plugin `p` newer than `have`.
fn new_releases<'a>(p: &'a MarketPlugin, have: &str) -> Vec<&'a Release> {
    p.releases.iter().filter(|r| newer(r.v, have)).collect()
}

fn updates_tab(app: &mut App, cv: &mut Cv, x: f32, y: f32, cw: f32, now: f64) -> f32 {
    let th = cv.th;
    let ups = updates(app);
    let busy: Vec<usize> = (0..market().len()).filter(|&i| matches!(state(app, i, now), State::Busy(_)) && installed(app, market()[i].id).is_some()).collect();
    let mut list = ups.clone();
    for b in busy {
        if !list.contains(&b) {
            list.push(b);
        }
    }
    if list.is_empty() {
        cv.boxed(x, y, cw, 150.0, 4.0, th.ink, th.tile);
        cv.boxed(x + cw / 2.0 - 20.0, y + 26.0, 40.0, 40.0, 3.0, th.ink, th.mint);
        cv.glyph("check", x + cw / 2.0 - 12.0, y + 34.0, 24.0, th.on_accent);
        cv.text_c(x, cw, y + 80.0, 22.0, 16.0, 700, th.ink, "Everything is up to date");
        cv.text_c(x, cw, y + 106.0, 19.0, 13.0, 400, th.slate, "New versions show up here with what changed in them.");
        return y + 150.0;
    }
    let mut y = y;
    if ups.len() > 1 {
        let wa = cv.measure(14.0, 600, "Update all") + 28.0;
        cv.text(x, y + 9.0, 22.0, 15.0, 600, th.ink, &format!("{} plugins have new versions", ups.len()));
        if cv.button("mupall2", x + cw - wa, y, wa, 40.0, "Update all", btn_update(cv)) {
            for &i in &ups {
                start(app, i, now);
            }
        }
        y += 40.0 + 16.0;
    }
    for &i in &list {
        let p = &market()[i];
        let have = installed(app, p.id).unwrap_or_default();
        let rels = new_releases(p, &have);
        // height: head, then each release with up to two notes
        let mut body = 0.0;
        for r in rels.iter().take(4) {
            body += 30.0 + r.notes.iter().take(2).count() as f32 * 20.0 + 10.0;
        }
        if rels.is_empty() {
            body = 30.0;
        }
        let ch = 18.0 + 52.0 + 14.0 + body + 12.0;
        cv.boxed(x, y, cw, ch, 4.0, th.ink, th.tile);
        icon_box(cv, p, x + 18.0, y + 18.0, 48.0);
        let tx = x + 18.0 + 48.0 + 14.0;
        cv.text(tx, y + 18.0, 23.0, 16.0, 700, th.ink, p.name);
        // 0.1.0 -> 0.2.0
        let from = format!("{have}");
        let fw = cv.measure(13.0, 600, &from);
        cv.text(tx, y + 44.0, 19.0, 13.0, 600, th.slate, &from);
        cv.px_icon("arrow-right", tx + fw + 6.0, y + 45.0, 16.0, th.slate);
        cv.text(tx + fw + 28.0, y + 44.0, 19.0, 13.0, 700, th.ink, p.version);
        let bw = 112.0;
        action_button(app, cv, &format!("mupd2_{i}"), i, x + cw - 18.0 - bw, y + 22.0, bw, 40.0, now);
        let wd = cv.measure(13.0, 600, "Details") + 24.0;
        if cv.button(&format!("mdet2_{i}"), x + cw - 18.0 - bw - 8.0 - wd, y + 22.0, wd, 40.0, "Details", cv.opt_btn(13.0)) {
            app.market_detail = Some(Detail { index: i, opened: None, closing: None, scroll: 0.0 });
        }
        // what is new
        let mut ry = y + 18.0 + 52.0 + 14.0;
        cv.fill(x + 18.0, ry - 8.0, cw - 36.0, 2.0, th.rule);
        if rels.is_empty() {
            cv.text(x + 18.0, ry + 4.0, 19.0, 13.0, 400, th.slate, "The author didn't write notes for this version.");
        }
        for r in rels.iter().take(4) {
            release_head(cv, r, x + 18.0, ry, cw - 36.0, true, false);
            ry += 30.0;
            for n in r.notes.iter().take(2) {
                cv.fill(x + 26.0, ry + 8.0, 4.0, 4.0, th.slate);
                cv.text_fit(x + 38.0, ry, 19.0, 13.0, 400, th.slate, n, cw - 36.0 - 30.0);
                ry += 20.0;
            }
            ry += 10.0;
        }
        y += ch + 16.0;
    }
    y
}

/// One release's first line: the version tag, its date, its title, and New / Installed marks.
fn release_head(cv: &mut Cv, r: &Release, x: f32, y: f32, w: f32, is_new: bool, is_have: bool) {
    let th = cv.th;
    let vw = cv.measure(12.0, 700, r.v) + 14.0;
    cv.boxed(x, y, vw, 22.0, 2.0, th.ink, if is_new { th.sun } else if is_have { th.mint } else { th.panel });
    cv.text(x + 7.0, y + 1.5, 17.0, 12.0, 700, if is_new || is_have { th.on_accent } else { th.ink }, r.v);
    let mut tx = x + vw + 10.0;
    if !r.date.is_empty() {
        let d = pretty_date(r.date);
        let dw = cv.text(tx, y + 2.0, 19.0, 13.0, 400, th.slate, &d);
        tx += dw + 12.0;
    }
    let mark = if is_have { "You have this" } else { "" };
    let mw = if mark.is_empty() { 0.0 } else { cv.measure(12.0, 600, mark) + 10.0 };
    let title = if r.title.is_empty() { "No notes for this version" } else { r.title };
    cv.text_fit(tx, y + 1.0, 20.0, 14.0, 600, if r.title.is_empty() { th.slate } else { th.ink }, title, x + w - tx - mw);
    if !mark.is_empty() {
        cv.text_r(x + w, y + 3.0, 17.0, 12.0, 600, th.mint, mark);
    }
}

/// The details window's release line: version tag, date, and New / You have this.
fn release_tag(cv: &mut Cv, r: &Release, x: f32, y: f32, w: f32, is_new: bool, is_have: bool) {
    let th = cv.th;
    let vw = cv.measure(12.0, 700, r.v) + 14.0;
    cv.boxed(x, y, vw, 22.0, 2.0, th.ink, if is_new { th.sun } else if is_have { th.mint } else { th.panel });
    cv.text(x + 7.0, y + 1.5, 17.0, 12.0, 700, if is_new || is_have { th.on_accent } else { th.ink }, r.v);
    if !r.date.is_empty() {
        cv.text(x + vw + 10.0, y + 2.0, 19.0, 13.0, 400, th.slate, &pretty_date(r.date));
    }
    let mark = if is_new { "New for you" } else if is_have { "You have this" } else { "" };
    if !mark.is_empty() {
        cv.text_r(x + w, y + 2.0, 19.0, 13.0, 700, if is_new { th.sun } else { th.mint }, mark);
    }
}

/// "2026-10-29" -> "29 Oct 2026".
fn pretty_date(d: &str) -> String {
    const M: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let p: Vec<&str> = d.split('-').collect();
    if p.len() == 3 {
        if let (Ok(m), Ok(day)) = (p[1].parse::<usize>(), p[2].parse::<u32>()) {
            if (1..=12).contains(&m) {
                return format!("{day} {} {}", M[m - 1], p[0]);
            }
        }
    }
    d.to_string()
}

/// The plugin details window, over everything.
pub fn dialog(app: &mut App, ui: &mut Ui, win: Rect) {
    let Some(mut d) = app.market_detail.take() else { return };
    if d.index >= market().len() {
        return;
    }
    let p = &market()[d.index];
    let th = app.theme();
    let ctx = ui.ctx().clone();
    let now = ctx.input(|i| i.time);
    tick(app, now, &ctx);
    let opened = *d.opened.get_or_insert(now);
    let mut a = ease(((now - opened) / 0.18) as f32);
    if let Some(t0) = d.closing {
        a = a.min(1.0 - ((now - t0) / 0.12) as f32);
        if a <= 0.0 {
            return;
        }
    }
    let a = a.clamp(0.0, 1.0);
    if a < 1.0 || d.closing.is_some() {
        ctx.request_repaint();
    }
    let mut close = false;
    let shade = ui.interact(win, egui::Id::new("md-blocker"), Sense::click_and_drag());
    {
        let mut cv = Cv::new(ui, win, win.min.to_vec2(), th, "mdshade");
        cv.fill(0.0, 0.0, win.width(), win.height(), with_alpha(th.shadow, 0.6 * a));
    }
    if shade.clicked() {
        close = true;
    }
    let dw = (win.width() - 120.0).min(860.0);
    let dh = (win.height() - 36.0 - 40.0).min(600.0);
    let dx = (win.min.x + (win.width() - dw) / 2.0).round();
    let dy = (win.min.y + 36.0 + (win.height() - 36.0 - dh) / 2.0 + 10.0 * (1.0 - a)).round();
    let rect = Rect::from_min_size(pos2(dx, dy), vec2(dw, dh));
    let mut cv = Cv::new(ui, win, rect.min.to_vec2(), th, "mdlg");
    cv.hit("md-body", 0.0, 0.0, dw, dh);
    cv.shadow(0.0, 0.0, dw, dh, 8.0, with_alpha(th.shadow, a));
    cv.boxed(0.0, 0.0, dw, dh, 4.0, th.ink, th.panel);
    // title bar
    cv.fill(4.0, 4.0, dw - 8.0, 46.0, th.shadow);
    let light = Color32::from_rgb(0xe4, 0xea, 0xf6);
    crate::app::draw_minnion(&mut cv, 16.0, 15.0, 24.0);
    cv.text(50.0, 16.0, 24.0, 18.0, 700, light, "Plugin details");
    let (cx, cy) = (dw - 4.0 - 10.0 - 34.0, 10.0);
    let r = cv.hit("mdclose", cx, cy, 34.0, 32.0);
    if r.hovered() {
        cv.fill(cx, cy, 34.0, 32.0, th.coral);
    }
    cv.border(cx, cy, 34.0, 32.0, 3.0, if r.hovered() { th.coral } else { light });
    let fg = if r.hovered() { Color32::WHITE } else { light };
    for k in 0..10 {
        cv.fill(cx + 11.0 + k as f32, cy + 10.0 + k as f32, 2.0, 2.0, fg);
        cv.fill(cx + 11.0 + (9 - k) as f32, cy + 10.0 + k as f32, 2.0, 2.0, fg);
    }
    if r.clicked() {
        close = true;
    }

    // head: icon, name, by, version and size, the button
    let i = d.index;
    let hy = 50.0 + 22.0;
    icon_box(&mut cv, p, 24.0, hy, 72.0);
    let tx = 24.0 + 72.0 + 18.0;
    cv.text_fit(tx, hy + 4.0, 30.0, 22.0, 700, th.ink, p.name, dw - tx - 200.0);
    byline(&mut cv, p, tx, hy + 38.0, dw - tx - 200.0);
    let have = installed(app, p.id);
    let mut facts = format!("{} · Version {}", cat_name(p.category), p.version);
    let sz = size_text(p.size);
    if !sz.is_empty() {
        facts.push_str(&format!(" · {sz}"));
    }
    if !p.published {
        facts.push_str(" · not in the online list yet");
    }
    if let Some(v) = &have {
        if newer(p.version, v) {
            facts.push_str(&format!(" · you have {v}"));
        }
    }
    cv.text_fit(tx, hy + 58.0, 19.0, 13.0, 400, th.slate, &facts, dw - tx - 200.0);
    action_button(app, &mut cv, "mdact", i, dw - 24.0 - 150.0, hy + 14.0, 150.0, 44.0, now);
    let hb = hy + 72.0 + 22.0;
    cv.fill(24.0, hb - 2.0, dw - 48.0, 2.0, th.rule);

    // left: about, what it adds, what you need
    let lw = ((dw - 48.0) * 0.52).floor();
    let mut y = hb + 18.0;
    cv.text(24.0, y, 20.0, 14.0, 700, th.ink, "About");
    y += 26.0;
    y += cv.para(24.0, y, lw - 16.0, 21.0, 14.0, 400, th.ink, p.about) + 16.0;
    cv.text(24.0, y, 20.0, 14.0, 700, th.ink, "Buttons it adds");
    y += 28.0;
    let mut chx = 24.0;
    for act in p.actions.iter().take(14) {
        let cwid = cv.measure(12.0, 600, act) + 16.0;
        if chx + cwid > 24.0 + lw - 16.0 {
            chx = 24.0;
            y += 30.0;
        }
        cv.boxed(chx, y, cwid, 24.0, 2.0, th.ink, th.tile);
        cv.text(chx + 8.0, y + 2.5, 17.0, 12.0, 600, th.ink, act);
        chx += cwid + 6.0;
    }
    if p.actions.len() > 14 {
        let more = format!("+{} more", p.actions.len() - 14);
        cv.text(chx, y + 3.0, 17.0, 12.0, 600, th.slate, &more);
    }
    y += 24.0 + 18.0;
    if !p.needs.is_empty() {
        cv.text(24.0, y, 20.0, 14.0, 700, th.ink, "You need");
        y += 26.0;
        for n in p.needs {
            cv.fill(30.0, y + 8.0, 4.0, 4.0, th.ink);
            y += cv.para(42.0, y, lw - 34.0, 19.0, 13.0, 400, th.ink, n) + 4.0;
        }
        y += 12.0;
    }
    if p.profile_pages > 0 {
        cv.boxed(24.0, y, lw - 16.0, 40.0, 3.0, th.ink, th.tile);
        cv.glyph("grid", 36.0, y + 12.0, 16.0, th.ink);
        cv.text(60.0, y + 10.0, 20.0, 13.0, 600, th.ink, &format!("Comes with a ready-made profile ({} pages)", p.profile_pages));
        // adds it as a new profile of its own, with its buttons already set up
        let o14 = cv.opt_btn(13.0);
        let bw = cv.measure(13.0, 600, "Add the profile") + 28.0;
        if cv.button("mkprofile", 24.0, y + 48.0, bw, 36.0, "Add the profile", o14) {
            app.start_market_profile(p.id, now);
        }
    }

    // right: what's new, newest first, scrolls
    let rx = 24.0 + lw + 16.0;
    let rw = dw - rx - 24.0;
    cv.fill(rx - 16.0, hb + 18.0, 2.0, dh - hb - 18.0 - 24.0, th.rule);
    cv.text(rx, hb + 18.0, 20.0, 14.0, 700, th.ink, "What's new");
    let list_top = hb + 18.0 + 30.0;
    let list_h = dh - list_top - 20.0;
    let area = cv.abs(rx, list_top, rw, list_h);
    if let Some(pp) = ctx.input(|inp| inp.pointer.hover_pos()) {
        if area.contains(pp) {
            d.scroll -= ctx.input(|inp| inp.smooth_scroll_delta.y);
        }
    }
    let clip = cv.clip_push(rx, list_top, rw, list_h);
    let mut ry = list_top - d.scroll;
    for r in p.releases {
        let is_new = have.as_deref().map(|v| newer(r.v, v)).unwrap_or(false);
        let is_have = have.as_deref() == Some(r.v);
        release_tag(&mut cv, r, rx, ry, rw, is_new, is_have);
        ry += 28.0;
        let title = if r.title.is_empty() { "No notes for this version" } else { r.title };
        ry += cv.para(rx, ry, rw, 20.0, 14.0, 600, if r.title.is_empty() { th.slate } else { th.ink }, title) + 4.0;
        for n in r.notes.iter().take(3) {
            cv.fill(rx + 8.0, ry + 8.0, 4.0, 4.0, th.slate);
            ry += cv.para(rx + 20.0, ry, rw - 20.0, 18.0, 12.0, 400, th.slate, n) + 4.0;
        }
        if r.notes.is_empty() && !r.title.is_empty() {
            ry += 2.0;
        }
        ry += 14.0;
    }
    if p.releases.is_empty() {
        cv.text(rx, ry, 19.0, 13.0, 400, th.slate, "No release notes yet.");
        ry += 20.0;
    }
    cv.clip_pop(clip);
    let content = ry + d.scroll - list_top;
    d.scroll = d.scroll.clamp(0.0, (content - list_h).max(0.0));
    if content > list_h {
        // a thin scroll bar
        let bar_h = (list_h * list_h / content).max(24.0);
        let bar_y = list_top + (list_h - bar_h) * (d.scroll / (content - list_h));
        cv.fill(rx + rw + 8.0, bar_y, 4.0, bar_h, with_alpha(th.ink, 0.4));
    }

    if ctx.input(|inp| inp.key_pressed(egui::Key::Escape)) {
        close = true;
    }
    if close && d.closing.is_none() {
        d.closing = Some(now);
    }
    app.market_detail = Some(d);
}
