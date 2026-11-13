//! Settings: a side list of tabs (with a search box) and, for the tab, its settings grouped in
//! cards. Animations: the side highlight glides and its icon hops, the cards slide in one after
//! another, segmented choices slide, a "Saved" tag pops up by a setting you change, rows light
//! up under the pointer, and theme pictures lift.
use crate::app::App;
use crate::ui::draw::{Cv, lerp, with_alpha};
use crate::ui::theme::Theme;
use egui::Color32;

const TABS: [(&str, &str, &str); 8] = [
    ("General", "gear", "How Minniondeck starts and closes."),
    ("Appearance", "sun", "Colours, motion and how buttons look."),
    ("Profiles and pages", "people", "When profiles switch and what opens first."),
    ("Phones", "phone", "How phones and tablets connect, and what they do."),
    ("Plugins", "bolt", "The plugins on this PC and how they are checked."),
    ("Backup", "folder", "Copies of your profiles, made for you."),
    ("Updates", "down", "New versions of Minniondeck and the phone app."),
    ("About", "star", "Minniondeck, its version and where it comes from."),
];

// The kinds of control a settings row can have; Text and None are drawn but no row uses them yet
#[allow(dead_code)]
enum Ctl {
    Switch(&'static str, bool),
    Seg(&'static str, &'static [&'static str], usize),
    Select(&'static str, &'static str),
    Button(&'static str, &'static str),
    Text(&'static str, &'static str),
    None,
}

struct Row {
    label: &'static str,
    desc: &'static str,
    soon: bool,
    ctl: Ctl,
}

/// Something a card shows besides plain rows.
#[derive(Clone, Copy, PartialEq)]
enum Extra {
    None,
    Themes,
    Rules,
    Plugins,
    UpdateCheck,
    Backups,
    About,
}

struct Section {
    title: &'static str,
    rows: Vec<Row>,
    extra: Extra,
}

fn r(label: &'static str, desc: &'static str, ctl: Ctl) -> Row {
    Row { label, desc, soon: false, ctl }
}
fn soon(label: &'static str, desc: &'static str, ctl: Ctl) -> Row {
    Row { label, desc, soon: true, ctl }
}
fn sec(title: &'static str, rows: Vec<Row>) -> Section {
    Section { title, rows, extra: Extra::None }
}
fn sec_x(title: &'static str, rows: Vec<Row>, extra: Extra) -> Section {
    Section { title, rows, extra }
}

fn sections(tab: usize, _app: &App) -> Vec<Section> {
    use Ctl::*;
    match tab {
        0 => vec![
            sec("Start-up", vec![
                r("Start with Windows", "Minniondeck starts in the tray when you sign in, so your buttons and phones are ready.", Switch("startWin", true)),
                r("Start minimised to the tray", "Open the window from the tray icon when you need it.", Switch("tray", false)),
            ]),
            sec("Window", vec![r("When you close the window", "Keep running so your buttons and phones still work.", Seg("close", &["Keep running", "Quit"], 0))]),
            sec("Language", vec![r("Language", "More languages are on the way. For now Minniondeck is in English.", Select("lang", "English"))]),
        ],
        1 => vec![
            sec_x("Theme", vec![], Extra::Themes),
            sec("Motion", vec![r("Animations", "Reduced keeps small effects. Off: nothing moves.", Seg("anim", &["Full", "Reduced", "Off"], 0))]),
            sec("Buttons", vec![
                r("Button size", "How much of the Deck the buttons fill.", Seg("tileSize", &["Small", "Medium", "Large"], 1)),
                r("Show button names", "Also hides names on phones.", Switch("names", true)),
                soon("Show the crew on the board", "The crew isn't on the Deck yet.", Switch("crew", false)),
            ]),
        ],
        2 => vec![
            sec_x("Switching", vec![r("Switch profiles automatically", "Switches when the app chosen on a profile comes to the front. Using Minniondeck itself never switches.", Switch("autoSwitch", true))], Extra::Rules),
            sec("Pages", vec![
                r("Show a Back button inside folders", "Pages inside a folder get a Back button in their first free spot, on the Deck and on phones.", Switch("back", true)),
                r("Open on start", "The page the Deck shows when Minniondeck starts.", Select("openOn", "Where I left off")),
            ]),
        ],
        3 => vec![
            sec("Connection", vec![
                r("Connect over", "Phones connect over Wi-Fi. USB and Both are coming soon.", Seg("conn", &["Wi-Fi", "USB", "Both"], 0)),
                r("Port", "Change this only if another app uses it.", Select("port", "8765")),
                r("Show a matching number when a new phone pairs", "Check the same number shows on both before you accept.", Switch("match", true)),
                r("Secure phone link", "Off: phones use the plain page, whose link is encrypted with a key in the QR code (no warning, nothing to install). On: the QR code opens the HTTPS page, which also stops anyone changing the page on its way; each phone installs this PC's certificate once.", Switch("secure", false)),
            ]),
            sec("On the phone", vec![
                r("Phones follow the page open on this PC", "", Switch("follow", true)),
                r("Keep phone screens awake", "", Switch("awake", true)),
                r("Vibrate when a button is pressed", "", Switch("vibe", true)),
                r("", "", Button("phones", "Manage phones")),
            ]),
        ],
        4 => vec![
            sec_x("Installed", vec![], Extra::Plugins),
            sec("Checks", vec![
                r("Check plugins for updates every day", "Plugins with a newer version show up in Marketplace › Updates.", Switch("plugUpd", true)),
                r("Developer mode", "Runs plugins that aren't signed by a trusted author, like ones you are building. Turn it on only for plugins you trust.", Switch("dev", false)),
            ]),
            sec("Files", vec![r("Plugins folder", "Each plugin has its own folder here.", Button("plugfolder", "Open folder"))]),
        ],
        5 => vec![
            sec("Automatic", vec![
                r("Back up when Minniondeck starts", "Only when something changed since the last backup. A backup is also made before every import and restore.", Switch("backup", true)),
                r("Backups to keep", "Older ones are removed.", Select("keep", "5")),
            ]),
            sec_x("Restore a backup", vec![], Extra::Backups),
            sec("Files", vec![
                r("Backups folder", "Backups are encrypted with this PC's own key: they open only in MinnionDeck on this Windows account.", Button("bkfolder", "Open folder")),
                r("", "", Button("bkexport", "Export all profiles…")),
            ]),
            sec("Move to another PC", vec![
                r("Password backup", "Ordinary backups don't open after Windows is installed again or on another PC. A password backup does, with the password you choose. Keep the password safe: without it the backup can't be opened.", Button("bkportable", "Save a password backup…")),
                r("", "", Button("bkportin", "Restore a password backup…")),
            ]),
        ],
        6 => vec![
            sec_x("Minniondeck", vec![
                r("Check for Minniondeck updates every day", "Looks once a day and tells you when a new version is out. Nothing downloads until you say so.", Switch("updDaily", true)),
                soon("Update channel", "", Seg("chan", &["Stable", "Beta"], 0)),
            ], Extra::UpdateCheck),
            sec("Phone app", vec![
                soon("Offer phone app updates when the desktop app updates", "", Switch("phoneUpd", false)),
                r("", "", Button("pushphone", "Send the phone app to my phones")),
            ]),
        ],
        _ => vec![sec_x("", vec![], Extra::About)],
    }
}

/// The panel colour a little deeper: towards the shadow in the dark theme, a faint grey in light.
fn shade(th: Theme, k: f32) -> Color32 {
    if th.dark { lerp(th.panel, th.shadow, k) } else { lerp(th.panel, th.ink, k * 0.18) }
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

pub fn draw(app: &mut App, cv: &mut Cv, w: f32, h: f32) -> f32 {
    let th = cv.th;
    let now = cv.now();
    let ctx = cv.ui.ctx().clone();
    cv.fill(0.0, 0.0, w, h.max(2000.0) + 600.0, th.panel);

    // ---- the side list: search, then the tabs -------------------------------------------
    let nav_w = 232.0;
    cv.fill(0.0, 0.0, nav_w, h.max(2000.0) + 600.0, shade(th, 0.25));
    cv.fill(nav_w, 0.0, 4.0, h.max(2000.0) + 600.0, th.ink);
    let mut q = app.settings_query.clone();
    cv.text_field("setq", 12.0, 14.0, nav_w - 24.0, 40.0, &mut q, 14.0, "");
    if q.is_empty() {
        cv.glyph("mkSearch", 24.0, 14.0 + 12.0, 16.0, th.slate);
        cv.text(48.0, 14.0 + 10.0, 20.0, 14.0, 400, th.slate, "Search settings");
    }
    if q != app.settings_query {
        app.settings_query = q;
        app.settings_at = now;
    }
    let searching = !app.settings_query.trim().is_empty();
    let ty0 = 68.0;
    let step = 44.0;
    // the highlight glides to the tab (0.2s) and its icon hops once
    let target = ty0 + app.settings_tab as f32 * step;
    let gy = ctx.animate_value_with_time(egui::Id::new("set-nav"), target, 0.2);
    if (gy - target).abs() > 0.5 {
        ctx.request_repaint();
    }
    if !searching {
        cv.shadow(12.0, gy, nav_w - 24.0, 40.0, 3.0, th.shadow);
        cv.boxed(12.0, gy, nav_w - 24.0, 40.0, 3.0, th.ink, th.mint);
    }
    for (i, (label, icon, _)) in TABS.iter().enumerate() {
        let y = ty0 + i as f32 * step;
        let resp = cv.hit(&format!("tab{i}"), 12.0, y, nav_w - 24.0, 40.0);
        let sel = app.settings_tab == i && !searching;
        let hv = ctx.animate_bool_with_time(egui::Id::new(("set-tab-hover", i)), resp.hovered() && !sel, 0.12);
        if hv > 0.0 {
            cv.fill(12.0, y, nav_w - 24.0, 40.0, with_alpha(th.hover, hv));
        }
        if hv > 0.0 && hv < 1.0 {
            ctx.request_repaint();
        }
        // on the highlight the words turn dark, as it slides under them
        let on = !searching && (gy - y).abs() < 20.0;
        let fg = if on { th.on_accent } else { th.ink };
        let mut hop = 0.0;
        if sel {
            let p = ((now - app.settings_at) / 0.3) as f32;
            if p < 1.0 {
                hop = -5.0 * (1.0 - (2.0 * crate::ui::widgets::steps(p, 3.0) - 1.0).abs());
                ctx.request_repaint();
            }
        }
        cv.glyph(icon, 26.0, y + 11.0 + hop, 18.0, fg);
        cv.text(54.0 + 2.0 * hv, y + 9.5, 21.0, 15.0, 600, fg, label);
        if resp.clicked() {
            if app.settings_tab != i || searching {
                app.settings_at = now;
            }
            app.settings_tab = i;
            app.settings_query.clear();
            app.scroll[4] = 0.0;
        }
    }

    // ---- the body -------------------------------------------------------------------------
    let x = nav_w + 4.0 + 28.0;
    let bw = (w - x - 14.0 - 32.0).min(860.0);
    let mut y;
    // the content slides in from the right and fades in, card after card
    let t0 = app.settings_at;
    if searching {
        cv.text(x, 22.0, 28.0, 20.0, 700, th.ink, "Search");
        cv.text(x, 54.0, 21.0, 15.0, 400, th.slate, &format!("Settings that mention \"{}\".", app.settings_query.trim()));
        y = 92.0;
        let ql = app.settings_query.trim().to_lowercase();
        let mut k = 0;
        for tab in 0..TABS.len() {
            let rows: Vec<Row> = sections(tab, app)
                .into_iter()
                .flat_map(|s| s.rows)
                .filter(|row| !row.label.is_empty() && (row.label.to_lowercase().contains(&ql) || row.desc.to_lowercase().contains(&ql)))
                .collect();
            if rows.is_empty() {
                continue;
            }
            let s = Section { title: TABS[tab].0, rows, extra: Extra::None };
            y += card(app, cv, tab, 100 + k, &s, x, y, bw, now, t0, k) + 18.0;
            k += 1;
        }
        if k == 0 {
            cv.dashed(x, y, bw, 110.0, 3.0, th.rule);
            cv.text_c(x, bw, y + 32.0, 22.0, 16.0, 700, th.ink, "Nothing found");
            cv.text_c(x, bw, y + 58.0, 19.0, 13.0, 400, th.slate, "Try another word, like phone, theme or backup.");
            y += 110.0;
        }
        return (y + 24.0).max(h);
    }
    let tab = app.settings_tab.min(TABS.len() - 1);
    let p = ease(((now - t0) / 0.25) as f32);
    let slide = 18.0 * (1.0 - p);
    cv.text(x + slide, 22.0, 28.0, 20.0, 700, th.ink, TABS[tab].0);
    cv.text(x + slide, 54.0, 21.0, 15.0, 400, th.slate, TABS[tab].2);
    y = 92.0;
    for (k, s) in sections(tab, app).iter().enumerate() {
        y += card(app, cv, tab, k, s, x, y, bw, now, t0, k + 1) + 18.0;
    }
    if now - t0 < 0.6 {
        ctx.request_repaint();
    }
    (y + 24.0).max(h)
}

/// A height for a row: label, "Coming soon", description, and a control beside or under them.
fn row_height(cv: &Cv, row: &Row, w: f32) -> f32 {
    let under = matches!(row.ctl, Ctl::Button(..) | Ctl::None) && !row.label.is_empty() || matches!(row.ctl, Ctl::Text(..));
    let cw = if under || matches!(row.ctl, Ctl::Button(..) | Ctl::None) { 0.0 } else { ctl_width(cv, &row.ctl) };
    let lab_w = w - if cw > 0.0 { cw + 24.0 } else { 0.0 };
    let mut lh = 0.0;
    if !row.label.is_empty() {
        lh += 22.0;
    }
    if row.soon {
        lh += 26.0;
    }
    if !row.desc.is_empty() {
        lh += 4.0 + 19.0 * cv.wrap_lines(lab_w, 13.0, 400, row.desc) as f32;
    }
    let ch = if cw > 0.0 { ctl_height(&row.ctl) } else { 0.0 };
    let mut hgt = lh.max(ch);
    if under || (matches!(row.ctl, Ctl::Button(..)) && row.label.is_empty()) {
        hgt += if lh > 0.0 { 12.0 } else { 0.0 } + 40.0;
    }
    16.0 + hgt + 16.0
}

fn ctl_width(cv: &Cv, ctl: &Ctl) -> f32 {
    match ctl {
        Ctl::Switch(..) => 60.0,
        Ctl::Seg(_, labels, _) => labels.iter().map(|l| cv.measure(14.0, 600, l) + 28.0).sum::<f32>() + 8.0,
        Ctl::Select(..) => 190.0,
        _ => 0.0,
    }
}

fn ctl_height(ctl: &Ctl) -> f32 {
    match ctl {
        Ctl::Switch(..) => 40.0,
        Ctl::Seg(..) => 44.0,
        Ctl::Select(..) => 40.0,
        _ => 0.0,
    }
}

fn extra_height(app: &App, e: Extra) -> f32 {
    match e {
        Extra::None => 0.0,
        Extra::Themes => 16.0 + 150.0 + 16.0,
        Extra::Rules => 16.0 + 40.0 + rule_count(app).clamp(1, 3) as f32 * 40.0 + 16.0,
        Extra::Plugins => {
            let n = plugin_rows(app).len().max(1);
            16.0 + 36.0 + n as f32 * 52.0 + 16.0 + 40.0 + 16.0
        }
        Extra::UpdateCheck => 16.0 + 64.0 + 16.0,
        Extra::Backups => 16.0 + 36.0 + app.backups.len().max(1) as f32 * 48.0 + 16.0 + 40.0 + 16.0,
        Extra::About => 16.0 + 250.0,
    }
}

/// A card: its title, the extra block, then the rows with rules between them. Returns its height.
#[allow(clippy::too_many_arguments)]
fn card(app: &mut App, cv: &mut Cv, tab: usize, k: usize, s: &Section, x: f32, y: f32, w: f32, now: f64, t0: f64, order: usize) -> f32 {
    let th = cv.th;
    let pad = 20.0;
    let iw = w - 2.0 * pad;
    let head = if s.title.is_empty() { 0.0 } else { 46.0 };
    let ex = extra_height(app, s.extra);
    let rows_h: f32 = s.rows.iter().map(|r| row_height(cv, r, iw)).sum();
    let hgt = head + ex + rows_h + if s.rows.is_empty() { 0.0 } else { 0.0 };
    // slide in: 24px from the right, 50ms after the card before it
    let p = ease((((now - t0) - order as f64 * 0.05) / 0.28) as f32);
    let dx = 24.0 * (1.0 - p);
    let x = x + dx;
    if p < 1.0 {
        cv.ui.ctx().request_repaint();
    }
    cv.shadow(x, y, w, hgt, 4.0, with_alpha(th.shadow, p));
    cv.boxed(x, y, w, hgt, 4.0, th.ink, th.tile);
    if !s.title.is_empty() {
        cv.text_spaced(x + pad, y + 18.0, 16.8, 12.0, 700, th.slate, &s.title.to_uppercase(), 0.8);
    }
    let mut yy = y + head;
    if s.extra != Extra::None {
        extra(app, cv, s.extra, x + pad, yy, iw, now);
        yy += ex;
    }
    for (n, row) in s.rows.iter().enumerate() {
        let rh = row_height(cv, row, iw);
        if n > 0 || head > 0.0 || ex > 0.0 {
            cv.fill(x + pad, yy, iw, 2.0, th.rule);
        }
        draw_row(app, cv, tab, k, n, x + pad, yy, iw, rh, row, now);
        yy += rh;
    }
    hgt
}

#[allow(clippy::too_many_arguments)]
fn draw_row(app: &mut App, cv: &mut Cv, tab: usize, k: usize, n: usize, x: f32, y: f32, w: f32, h: f32, row: &Row, now: f64) {
    let th = cv.th;
    let ctx = cv.ui.ctx().clone();
    let key_id = format!("c{tab}_{k}_{n}");
    // the row lights up under the pointer
    let hit = cv.hit(&format!("{key_id}_row"), x - 12.0, y + 2.0, w + 24.0, h - 4.0);
    let hv = ctx.animate_bool_with_time(egui::Id::new(("set-row", tab, k, n)), hit.hovered(), 0.15);
    if hv > 0.0 {
        cv.fill(x - 12.0, y + 2.0, w + 24.0, h - 4.0, with_alpha(th.hover, 0.55 * hv));
        if hv < 1.0 {
            ctx.request_repaint();
        }
    }
    let beside = matches!(row.ctl, Ctl::Switch(..) | Ctl::Seg(..) | Ctl::Select(..));
    let cw = if beside { ctl_width(cv, &row.ctl) } else { 0.0 };
    let lab_w = w - if cw > 0.0 { cw + 24.0 } else { 0.0 };
    let mut lh = 0.0;
    if !row.label.is_empty() {
        lh += 22.0;
    }
    if row.soon {
        lh += 26.0;
    }
    if !row.desc.is_empty() {
        lh += 4.0 + 19.0 * cv.wrap_lines(lab_w, 13.0, 400, row.desc) as f32;
    }
    let ch = if beside { ctl_height(&row.ctl) } else { 0.0 };
    let block = lh.max(ch);
    let mut ty = y + 16.0 + if beside && ch > lh { (ch - lh) / 2.0 } else { 0.0 };
    if !row.label.is_empty() {
        let col = if row.soon { th.slate } else { th.ink };
        let lw = cv.measure(16.0, 700, row.label).min(lab_w);
        cv.text_fit(x, ty, 22.0, 16.0, 700, col, row.label, lab_w);
        // "Saved": pops in by the label when this setting changes, fades out after a second
        if let Some((key, t0)) = app.settings_saved {
            if ctl_key(&row.ctl) == Some(key) {
                let a = now - t0;
                if a < 1.6 {
                    let pop = if a < 0.15 { 0.6 + 0.4 * ease((a / 0.15) as f32) } else { 1.0 };
                    let alpha = if a > 1.2 { 1.0 - ((a - 1.2) / 0.4) as f32 } else { 1.0 };
                    let sw = (cv.measure(12.0, 700, "Saved") + 30.0) * pop;
                    let sx = x + lw + 12.0;
                    cv.fill(sx, ty + 1.0, sw, 20.0, with_alpha(th.mint, alpha));
                    if pop > 0.9 {
                        cv.glyph("check", sx + 6.0, ty + 3.0, 14.0, with_alpha(th.on_accent, alpha));
                        cv.text(sx + 24.0, ty + 2.0, 17.0, 12.0, 700, with_alpha(th.on_accent, alpha), "Saved");
                    }
                    ctx.request_repaint();
                }
            }
        }
        ty += 22.0;
    }
    if row.soon {
        let tw = cv.measure(12.0, 600, "Coming soon") + 16.0;
        cv.border(x, ty + 2.0, tw, 22.0, 2.0, th.slate);
        cv.text(x + 8.0, ty + 3.0, 19.0, 12.0, 600, th.slate, "Coming soon");
        ty += 26.0;
    }
    if !row.desc.is_empty() {
        cv.para(x, ty + 4.0, lab_w, 19.0, 13.0, 400, th.slate, row.desc);
    }
    // the control: beside the words, or under them
    let cy = y + 16.0 + (block - ch) / 2.0;
    let cx = x + w - cw;
    let under_y = y + 16.0 + lh + if lh > 0.0 { 12.0 } else { 0.0 };
    match &row.ctl {
        Ctl::Switch(key, default) => {
            let on = app.st.get(key).map(|v| *v == 1).unwrap_or(*default);
            if !row.soon && cv.switch(&key_id, cx, cy, on) {
                // Developer mode lets plugins nobody vouched for run: asked before it goes on
                let sure = *key != "dev" || on || crate::backend::actions::confirm(
                    "Turn on Developer mode?",
                    "Plugins that aren't signed by an author you trust will run, with everything you can do on this PC.\n\nTurn it on only while you build or test plugins you trust.",
                );
                if sure {
                    app.st.insert(key, if on { 0 } else { 1 });
                    app.settings_saved = Some((key, now));
                }
            } else if row.soon {
                cv.fill(cx, cy, 60.0, 40.0, th.panel);
                cv.border(cx, cy, 60.0, 40.0, 4.0, th.rule);
            }
        }
        Ctl::Seg(key, labels, default) => {
            let cur = app.st.get(key).copied().unwrap_or(*default);
            seg(app, cv, &key_id, key, labels, cur, cx, cy, cw, row.soon, now);
        }
        Ctl::Select(_, text) => {
            cv.select_box(&key_id, cx, cy, 190.0, 40.0, text, 14.0, 3.0, 10.0, false);
        }
        Ctl::Button(id, label) => {
            let o = cv.opt_btn(14.0);
            let bwid = cv.measure(14.0, 600, label) + 32.0;
            if cv.button(&key_id, x, under_y, bwid, 40.0, label, o) {
                button_action(app, id, now);
            }
        }
        Ctl::Text(key, ph) => {
            let mut v = app.settings_text.get(*key).cloned().unwrap_or_default();
            let before = v.clone();
            cv.text_field(&key_id, x, under_y, (w * 0.6).min(420.0), 40.0, &mut v, 14.0, ph);
            if v != before {
                app.settings_text.insert(key, v);
                app.settings_saved = Some((key, now));
            }
        }
        Ctl::None => {}
    }
}

fn ctl_key(c: &Ctl) -> Option<&'static str> {
    match c {
        Ctl::Switch(k, _) | Ctl::Seg(k, _, _) | Ctl::Select(k, _) | Ctl::Text(k, _) => Some(k),
        _ => None,
    }
}

/// A segmented choice whose mint pill slides to the option picked.
#[allow(clippy::too_many_arguments)]
fn seg(app: &mut App, cv: &mut Cv, id: &str, key: &'static str, labels: &[&str], cur: usize, x: f32, y: f32, w: f32, disabled: bool, now: f64) {
    let th = cv.th;
    let ctx = cv.ui.ctx().clone();
    cv.fill(x, y, w, 44.0, if disabled { th.panel } else { th.tile });
    cv.border(x, y, w, 44.0, 3.0, if disabled { th.rule } else { th.ink });
    let widths: Vec<f32> = labels.iter().map(|l| cv.measure(14.0, 600, l) + 28.0).collect();
    let lefts: Vec<f32> = widths.iter().scan(x + 4.0, |acc, w| {
        let l = *acc;
        *acc += w;
        Some(l)
    }).collect();
    let tx = ctx.animate_value_with_time(egui::Id::new(("seg-x", id)), lefts[cur.min(labels.len() - 1)], 0.18);
    let tw = ctx.animate_value_with_time(egui::Id::new(("seg-w", id)), widths[cur.min(labels.len() - 1)], 0.18);
    if (tx - lefts[cur.min(labels.len() - 1)]).abs() > 0.5 {
        ctx.request_repaint();
    }
    if !disabled {
        cv.fill(tx, y + 4.0, tw, 36.0, th.mint);
    }
    for (i, l) in labels.iter().enumerate() {
        let resp = cv.hit(&format!("{id}_{i}"), lefts[i], y + 4.0, widths[i], 36.0);
        let under = !disabled && tx < lefts[i] + widths[i] / 2.0 && tx + tw > lefts[i] + widths[i] / 2.0;
        if resp.hovered() && i != cur && !disabled {
            cv.fill(lefts[i], y + 4.0, widths[i], 36.0, with_alpha(th.hover, 0.8));
        }
        let fg = if disabled { th.slate } else if under { th.on_accent } else { th.ink };
        cv.text_c(lefts[i], widths[i], y + 4.0 + 8.0, 20.0, 14.0, 600, fg, l);
        if resp.clicked() && !disabled && i != cur {
            app.st.insert(key, i);
            app.settings_saved = Some((key, now));
            if key == "theme" {
                app.dark = i != 1;
            }
        }
    }
}

fn button_action(app: &mut App, id: &str, now: f64) {
    match id {
        "phones" => app.pending_view = Some(crate::app::View::Phones),
        "market" => app.pending_view = Some(crate::app::View::Marketplace),
        "pushphone" => app.say("Sending the phone app isn't connected yet", now),
        "bkexport" => app.start_export_all(now),
        "bkportable" | "bkportin" => app.portable = Some(crate::views::portable_dialog::PortableAsk::new(id == "bkportable")),
        "bkfolder" | "plugfolder" => {
            let dir = if id == "bkfolder" { app.lib.paths.backups.clone() } else { app.lib.paths.root.join("plugins") };
            let _ = std::fs::create_dir_all(&dir);
            if let Err(e) = crate::backend::actions::open_path(&dir) {
                app.say(&e, now);
            }
        }
        _ => app.say("Opening folders isn't connected yet", now),
    }
}

/// The blocks that are not plain rows.
fn extra(app: &mut App, cv: &mut Cv, e: Extra, x: f32, y: f32, w: f32, now: f64) {
    let th = cv.th;
    let ctx = cv.ui.ctx().clone();
    match e {
        Extra::Themes => {
            // three pictures of the window: System (half and half), Light, Dark
            let cur = app.st.get("theme").copied().unwrap_or(if app.dark { 2 } else { 1 });
            let gap = 16.0;
            let tw = ((w - 2.0 * gap) / 3.0).min(220.0);
            for (i, name) in ["System", "Light", "Dark"].iter().enumerate() {
                let tx = x + i as f32 * (tw + gap);
                let resp = cv.hit(&format!("theme{i}"), tx, y + 16.0, tw, 150.0);
                let hv = ctx.animate_bool_with_time(egui::Id::new(("theme-hover", i)), resp.hovered(), 0.15);
                if hv > 0.0 && hv < 1.0 {
                    ctx.request_repaint();
                }
                let sel = cur == i;
                let ty = y + 16.0 - 3.0 * hv;
                let ph = 112.0;
                cv.shadow(tx, ty, tw, ph, 3.0 + 2.0 * hv, th.shadow);
                if i == 0 {
                    let half = (tw / 2.0).round();
                    let clip = cv.clip_push(tx, ty, half, ph);
                    mini(cv, tx, ty, tw, ph, Theme::light());
                    cv.clip_pop(clip);
                    let clip = cv.clip_push(tx + half, ty, tw - half, ph);
                    mini(cv, tx, ty, tw, ph, Theme::dark());
                    cv.clip_pop(clip);
                } else {
                    mini(cv, tx, ty, tw, ph, if i == 1 { Theme::light() } else { Theme::dark() });
                }
                cv.border(tx, ty, tw, ph, 3.0, th.ink);
                if sel {
                    cv.border(tx - 6.0, ty - 6.0, tw + 12.0, ph + 12.0, 3.0, th.mint);
                }
                // the name, with a round mark
                let ny = y + 16.0 + ph + 12.0;
                cv.boxed(tx, ny + 2.0, 18.0, 18.0, 3.0, th.ink, if sel { th.mint } else { th.tile });
                if sel {
                    cv.fill(tx + 6.0, ny + 8.0, 6.0, 6.0, th.on_accent);
                }
                cv.text(tx + 28.0, ny, 22.0, 15.0, if sel { 700 } else { 600 }, th.ink, name);
                if resp.clicked() && !sel {
                    app.st.insert("theme", i);
                    app.settings_saved = Some(("theme", now));
                    app.dark = i != 1;
                }
            }
        }
        Extra::Rules => {
            let yy = y + 16.0;
            cv.boxed(x, yy, w, 40.0 + rule_count(app).clamp(1, 3) as f32 * 40.0, 3.0, th.ink, th.panel);
            cv.fill(x + 3.0, yy + 3.0, w - 6.0, 34.0, shade(th, 0.3));
            cv.text(x + 14.0, yy + 10.0, 20.0, 13.0, 700, th.ink, "When this app is in front");
            cv.text(x + w / 2.0, yy + 10.0, 20.0, 13.0, 700, th.ink, "Switch to");
            let rules: Vec<(String, String)> = app
                .sample
                .profiles
                .iter()
                .filter(|p| p.auto != 0)
                .map(|p| (crate::backend::model::AUTO_CHOICES[p.auto].trim_end_matches(" is in front").to_string(), p.name.clone()))
                .collect();
            if rules.is_empty() {
                cv.text(x + 14.0, yy + 52.0, 20.0, 14.0, 400, th.slate, "No rules yet. Pick an app for a profile in Profiles › Edit.");
            }
            for (j, (app_name, prof)) in rules.iter().take(3).enumerate() {
                let ry = yy + 40.0 + j as f32 * 40.0;
                if j > 0 {
                    cv.fill(x + 3.0, ry, w - 6.0, 2.0, th.rule);
                }
                cv.glyph("app", x + 14.0, ry + 12.0, 16.0, th.ink);
                cv.text(x + 40.0, ry + 10.0, 20.0, 14.0, 600, th.ink, app_name);
                cv.px_icon("arrow-right", x + w / 2.0 - 30.0, ry + 12.0, 16.0, th.slate);
                cv.text(x + w / 2.0, ry + 10.0, 20.0, 14.0, 600, th.ink, prof);
            }
        }
        Extra::Plugins => {
            let yy = y + 16.0;
            // the plugins that run (or, in a screenshot, the ones the Marketplace lists as installed)
            let list: Vec<PlugRow> = plugin_rows(app);
            let n = list.len().max(1);
            let th_h = 36.0 + n as f32 * 52.0;
            cv.boxed(x, yy, w, th_h, 3.0, th.ink, th.panel);
            cv.fill(x + 3.0, yy + 3.0, w - 6.0, 30.0, shade(th, 0.3));
            let cols = [0.0, 0.42, 0.62, 0.8];
            for (c, label) in ["Plugin", "Version", "Signed by", "Status"].iter().enumerate() {
                cv.text(x + 14.0 + cols[c] * (w - 28.0), yy + 8.0, 20.0, 13.0, 700, th.ink, label);
            }
            if list.is_empty() {
                cv.text(x + 14.0, yy + 50.0, 20.0, 14.0, 400, th.slate, "No plugins yet. Find some in the Marketplace.");
            }
            for (j, row) in list.iter().enumerate() {
                let ry = yy + 36.0 + j as f32 * 52.0;
                if j > 0 {
                    cv.fill(x + 3.0, ry, w - 6.0, 2.0, th.rule);
                }
                // a click on a plugin opens its window: settings, turn off, remove
                if app.plug.running() {
                    let hit = cv.hit(&format!("plugrow{j}"), x + 3.0, ry, w - 6.0, 52.0);
                    if hit.hovered() {
                        let top = if j > 0 { 2.0 } else { 0.0 };
                        cv.fill(x + 3.0, ry + top, w - 6.0, 52.0 - top, shade(th, 0.15));
                    }
                    if hit.clicked() {
                        app.plug_dialog = Some(crate::views::plugin_dialog::PlugDialog::new(&row.id));
                    }
                }
                let mp = crate::generated::market_data::MARKET.iter().find(|p| p.id == row.id);
                let icon = crate::backend::catalog::plugins().iter().find(|p| p.id == row.id).map(|p| p.icon).unwrap_or("bolt");
                let colour = mp.map(|p| p.colour).unwrap_or("mint");
                cv.boxed(x + 14.0, ry + 10.0, 32.0, 32.0, 2.0, th.ink, th.accent(colour));
                cv.icon(icon, x + 14.0 + 8.0, ry + 18.0, 16.0, th.on_accent);
                cv.text_fit(x + 56.0, ry + 15.0, 21.0, 15.0, 600, th.ink, &row.name, cols[1] * (w - 28.0) - 50.0);
                cv.text(x + 14.0 + cols[1] * (w - 28.0), ry + 16.0, 20.0, 14.0, 400, th.ink, &row.version);
                cv.text_fit(x + 14.0 + cols[2] * (w - 28.0), ry + 16.0, 20.0, 14.0, 400, th.ink, &row.signer, 0.17 * (w - 28.0));
                let behind = mp.map(|p| crate::views::marketplace::newer(p.version, &row.version)).unwrap_or(false);
                let (dot, word) = match row.state {
                    "ready" if behind => (th.sun, "Update ready"),
                    "ready" => (th.mint, "Running"),
                    "waking" => (th.sun, "Starting"),
                    "off" => (th.slate, "Turned off"),
                    _ => (th.coral, "Stopped"),
                };
                let sx = x + 14.0 + cols[3] * (w - 28.0);
                cv.fill(sx, ry + 22.0, 8.0, 8.0, dot);
                cv.text(sx + 14.0, ry + 16.0, 20.0, 14.0, 600, th.ink, word);
            }
            let by = yy + th_h + 16.0;
            let o = cv.opt_btn(14.0);
            let mut bx = x;
            for (i, l) in ["Open the Marketplace", "Install a plugin file…", "Reload plugins", "Trust an author…"].iter().enumerate() {
                let bwid = cv.measure(14.0, 600, l) + 32.0;
                if cv.button(&format!("plug{i}"), bx, by, bwid, 40.0, l, o) {
                    match i {
                        0 => app.pending_view = Some(crate::app::View::Marketplace),
                        1 => app.start_plugin_pick(),
                        3 => app.start_author_pick(),
                        _ if app.plug.running() => {
                            std::thread::spawn(|| {
                                let _ = crate::backend::plugins::reload(None);
                            });
                            app.say("Reloading plugins…", now);
                        }
                        _ => app.say("Plugins reloaded", now),
                    }
                }
                bx += bwid + 10.0;
            }
        }
        Extra::Backups => {
            let yy = y + 16.0;
            if now - app.backups_at > 2.0 {
                app.refresh_backups(now);
            }
            ctx.request_repaint_after(std::time::Duration::from_secs(2));
            let list = app.backups.clone();
            let n = list.len().max(1);
            let th_h = 36.0 + n as f32 * 48.0;
            cv.boxed(x, yy, w, th_h, 3.0, th.ink, th.panel);
            cv.fill(x + 3.0, yy + 3.0, w - 6.0, 30.0, shade(th, 0.3));
            for (label, cx) in [("Made", 0.0), ("Size", 0.45)] {
                cv.text(x + 14.0 + cx * (w - 28.0), yy + 8.0, 20.0, 13.0, 700, th.ink, label);
            }
            if list.is_empty() {
                cv.text(x + 14.0, yy + 48.0, 20.0, 14.0, 400, th.slate, "No backups yet. One is made when Minniondeck starts, and before every import and restore.");
            }
            let o = cv.opt_btn(13.0);
            for (j, b) in list.iter().enumerate() {
                let ry = yy + 36.0 + j as f32 * 48.0;
                if j > 0 {
                    cv.fill(x + 3.0, ry, w - 6.0, 2.0, th.rule);
                }
                cv.text(x + 14.0, ry + 14.0, 20.0, 14.0, 600, th.ink, &crate::backend::backups::local_text(b.created_ms));
                cv.text(x + 14.0 + 0.45 * (w - 28.0), ry + 14.0, 20.0, 14.0, 400, th.ink, &crate::backend::backups::size_text(b.bytes));
                let confirm = app.backup_confirm == Some(j);
                let mut ro = o;
                if confirm {
                    ro.bg = th.coral_dark;
                    ro.bg_hover = th.coral;
                    ro.fg = Color32::WHITE;
                }
                if cv.button(&format!("bkrestore{j}"), x + w - 14.0 - 96.0, ry + 5.0, 96.0, 38.0, if confirm { "Sure?" } else { "Restore" }, ro) {
                    if confirm {
                        app.backup_confirm = None;
                        app.start_restore(b.name.clone());
                    } else {
                        app.backup_confirm = Some(j);
                    }
                }
            }
            let by = yy + th_h + 16.0;
            let bwid = cv.measure(14.0, 600, "Back up now") + 32.0;
            let o14 = cv.opt_btn(14.0);
            if cv.button("bknow", x, by, bwid, 40.0, "Back up now", o14) {
                app.start_backup();
            }
        }
        Extra::UpdateCheck => {
            // the version you have, what the last check found, and the one button for the next step
            use crate::app_updates::UpdState;
            let yy = y + 16.0;
            cv.boxed(x, yy, w, 64.0, 3.0, th.ink, th.panel);
            crate::app::draw_minnion(cv, x + 14.0, yy + 12.0, 40.0);
            cv.text(x + 66.0, yy + 12.0, 22.0, 16.0, 700, th.ink, &format!("Minniondeck {}", crate::backend::updates::current_version()));
            let state = app.upd.state.clone();
            let (label, disabled) = match &state {
                UpdState::Available(_) => ("Download".to_string(), false),
                UpdState::Downloading => ("Cancel".to_string(), false),
                UpdState::Ready => ("Install now".to_string(), false),
                UpdState::Checking(_) => ("Check for updates".to_string(), true),
                _ => ("Check for updates".to_string(), !app.tray.on()),
            };
            let bwid = cv.measure(14.0, 600, "Check for updates").max(cv.measure(14.0, 600, &label)) + 32.0;
            let text_w = w - 66.0 - bwid - 28.0;
            match &state {
                UpdState::Checking(t0) => {
                    let dots = (((now - t0) * 4.0) as usize) % 4;
                    cv.text(x + 66.0, yy + 36.0, 19.0, 13.0, 400, th.slate, &format!("Checking{}", ".".repeat(dots)));
                    let p = (((now - t0) / 3.0) as f32).fract();
                    cv.fill(x + w - 14.0 - bwid, yy + 54.0, bwid * p, 4.0, th.mint);
                }
                UpdState::Latest => {
                    cv.glyph("check", x + 66.0, yy + 38.0, 14.0, th.mint);
                    cv.text(x + 86.0, yy + 36.0, 19.0, 13.0, 600, th.mint, "You have the newest version.");
                }
                UpdState::Available(i) => {
                    let size = if i.size > 0 { format!(" ({})", crate::backend::backups::size_text(i.size)) } else { String::new() };
                    cv.text_fit(x + 66.0, yy + 36.0, 19.0, 13.0, 600, th.sun, &format!("Version {} is ready{size}.", i.latest), text_w);
                }
                UpdState::Downloading => {
                    let p = app.upd.progress().clamp(0.0, 1.0);
                    cv.text(x + 66.0, yy + 36.0, 19.0, 13.0, 400, th.slate, &format!("Downloading… {}%", (p * 100.0) as u32));
                    cv.fill(x + w - 14.0 - bwid, yy + 54.0, bwid * p, 4.0, th.mint);
                }
                UpdState::Ready => {
                    cv.text(x + 66.0, yy + 36.0, 19.0, 13.0, 600, th.mint, "Downloaded and checked. Minniondeck closes to install it.");
                }
                UpdState::Error(e) => {
                    cv.text_fit(x + 66.0, yy + 36.0, 19.0, 13.0, 400, th.coral, e, text_w);
                }
                UpdState::Idle => {
                    let last = app.settings.get_u64("updChecked");
                    let s = if last == 0 { "Not checked yet.".to_string() } else { format!("Last checked {}.", crate::backend::backups::local_text(last)) };
                    cv.text(x + 66.0, yy + 36.0, 19.0, 13.0, 400, th.slate, &s);
                }
            }
            let mut o = cv.opt_btn(14.0);
            o.disabled = disabled;
            if cv.button("updcheck", x + w - 14.0 - bwid, yy + 12.0, bwid, 40.0, &label, o) {
                match &state {
                    UpdState::Available(_) => app.start_update_download(),
                    UpdState::Downloading => app.cancel_update_download(),
                    UpdState::Ready => app.install_update(&ctx, now),
                    _ => app.start_update_check(false, now),
                }
            }
        }
        Extra::About => {
            let yy = y + 16.0;
            // the minnion bobs gently
            let bob = ((now * 2.0).sin() * 3.0) as f32;
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
            cv.boxed(x, yy, 112.0, 112.0, 3.0, th.ink, th.ground);
            crate::app::draw_minnion(cv, x + 16.0, yy + 16.0 + bob.round(), 80.0);
            let tx = x + 112.0 + 24.0;
            cv.text(tx, yy + 8.0, 34.0, 26.0, 700, th.ink, "Minniondeck");
            cv.text(tx, yy + 46.0, 20.0, 14.0, 600, th.slate, &format!("Version {} · Windows", env!("CARGO_PKG_VERSION")));
            cv.para(tx, yy + 74.0, w - (tx - x), 20.0, 14.0, 400, th.ink, "Turn your PC, phone or tablet into a deck of buttons: shortcuts, apps, sound and more.");
            let mut bx = x;
            let by = yy + 140.0;
            let o = cv.opt_btn(14.0);
            for (i, l) in ["Website", "GitHub", "What's new", "Licences"].iter().enumerate() {
                let bwid = cv.measure(14.0, 600, l) + 32.0;
                if cv.button(&format!("about{i}"), bx, by, bwid, 40.0, l, o) {
                    app.say("Opening links isn't connected yet", now);
                }
                bx += bwid + 10.0;
            }
            cv.fill(x, by + 60.0, w, 2.0, th.rule);
            cv.text(x, by + 76.0, 20.0, 13.0, 600, th.ink, "Phone app");
            cv.text(x + 120.0, by + 76.0, 20.0, 13.0, 400, th.slate, "Android, from the website");
        }
        Extra::None => {}
    }
}

fn rule_count(app: &App) -> usize {
    app.sample.profiles.iter().filter(|p| p.auto != 0).count()
}

/// A small picture of the window in theme `t`: title bar, rail, a few buttons.
fn mini(cv: &mut Cv, x: f32, y: f32, w: f32, h: f32, t: Theme) {
    cv.fill(x, y, w, h, t.panel);
    cv.fill(x, y, w, 14.0, t.shadow);
    cv.fill(x + 6.0, y + 5.0, 4.0, 4.0, t.mint);
    cv.fill(x, y + 14.0, 22.0, h - 14.0, t.tile);
    cv.fill(x + 22.0, y + 14.0, 2.0, h - 14.0, t.ink);
    for k in 0..3 {
        cv.fill(x + 6.0, y + 22.0 + k as f32 * 16.0, 10.0, 10.0, if k == 0 { t.mint } else { with_alpha(t.ink, 0.5) });
    }
    cv.dots(x + 24.0, y + 14.0, w - 24.0, h - 14.0, t.ground, t.ground_dot);
    let cols = ["sky", "lilac", "peach", "mint", "sun", "coral"];
    let cell = ((w - 24.0 - 24.0 - 2.0 * 8.0) / 3.0).min((h - 14.0 - 24.0 - 8.0) / 2.0).floor();
    let gx = x + 24.0 + (w - 24.0 - (3.0 * cell + 16.0)) / 2.0;
    let gy = y + 14.0 + (h - 14.0 - (2.0 * cell + 8.0)) / 2.0;
    for (k, c) in cols.iter().enumerate() {
        let (bx, by) = (gx + (k % 3) as f32 * (cell + 8.0), gy + (k / 3) as f32 * (cell + 8.0));
        cv.fill(bx + 2.0, by + 2.0, cell, cell, t.shadow);
        cv.fill(bx, by, cell, cell, t.accent(c));
        cv.border(bx, by, cell, cell, 2.0, t.ink);
    }
}

#[allow(dead_code)]
fn _unused(_: Color32) {}

/// A line of the Plugins table.
struct PlugRow {
    id: String,
    name: String,
    version: String,
    signer: String,
    state: &'static str,
}

fn plugin_rows(app: &App) -> Vec<PlugRow> {
    use crate::backend::signing::Trust;
    if app.plug.running() {
        return app
            .plug
            .rows
            .iter()
            .map(|p| PlugRow {
                id: p.id.clone(),
                name: p.name.clone(),
                version: p.version.clone(),
                signer: match &p.signature {
                    Trust::Trusted { author } | Trust::User { author } | Trust::Untrusted { author } => author.clone(),
                    Trust::Unsigned => "Not signed".into(),
                    Trust::Bad { .. } => "Damaged".into(),
                },
                state: match p.state.as_str() {
                    "ready" => "ready",
                    "waking" => "waking",
                    "off" => "off",
                    _ => "dizzy",
                },
            })
            .collect();
    }
    app.market_installed
        .iter()
        .map(|(id, v)| PlugRow {
            id: id.to_string(),
            name: crate::generated::market_data::MARKET.iter().find(|p| p.id == *id).map(|p| p.name.to_string()).unwrap_or(id.to_string()),
            version: v.clone(),
            signer: "Minniondeck".into(),
            state: "ready",
        })
        .collect()
}
