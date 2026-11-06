//! The button press animations of 05-tiles.css (`.anim.a-*`), evaluated by hand: each is a CSS
//! keyframe animation with a `steps(n, end)` timing, 400ms by default.

pub const NAMES: [&str; 16] = [
    "None", "Press", "Pulse", "Pop", "Flash", "Bounce", "Jelly", "Wiggle", "Shake", "Heart", "Spin", "Ring", "Rainbow", "Scan", "Fill", "On air",
];

pub const DURATION: f64 = 0.4;

/// What an animation does to a button at one moment.
#[derive(Clone, Copy)]
pub struct Xf {
    pub dx: f32,
    pub dy: f32,
    pub sx: f32,
    pub sy: f32,
    /// Brightness boost 0..1 (flash) and hue turn 0..1 (rainbow).
    pub flash: f32,
    pub hue: f32,
    /// Inset ring thickness (ring), overlay band 0..1 position (scan), overlay height 0..1 (fill).
    pub ring: f32,
    pub scan: Option<f32>,
    pub fill: f32,
    pub onair: bool,
    pub shadow_off: bool,
}

impl Xf {
    pub fn id() -> Xf {
        Xf { dx: 0.0, dy: 0.0, sx: 1.0, sy: 1.0, flash: 0.0, hue: 0.0, ring: 0.0, scan: None, fill: 0.0, onair: false, shadow_off: false }
    }
}

fn steps(p: f32, n: f32) -> f32 {
    if p >= 1.0 { 1.0 } else { (p.clamp(0.0, 1.0) * n).floor() / n }
}

/// Piecewise-linear keyframes, sampled at the step the animation is on.
fn key(frames: &[(f32, f32)], p: f32) -> f32 {
    for w in frames.windows(2) {
        if p <= w[1].0 {
            let (a, b) = (w[0], w[1]);
            let t = if b.0 > a.0 { (p - a.0) / (b.0 - a.0) } else { 0.0 };
            return a.1 + (b.1 - a.1) * t;
        }
    }
    frames.last().map(|f| f.1).unwrap_or(0.0)
}

/// `t` is seconds since the animation started; returns None once it is over.
pub fn eval(kind: usize, t: f64) -> Option<Xf> {
    if kind == 0 || t >= DURATION {
        return None;
    }
    let p = (t / DURATION) as f32;
    let mut x = Xf::id();
    match kind {
        1 => {
            let q = steps(p, 2.0);
            let k = if q >= 0.5 && q < 1.0 { 1.0 } else { 0.0 };
            x.dx = 4.0 * k;
            x.dy = 4.0 * k;
            x.shadow_off = k > 0.0;
        }
        2 => {
            let s = key(&[(0.0, 1.0), (0.5, 1.07), (1.0, 1.0)], steps(p, 4.0));
            x.sx = s;
            x.sy = s;
        }
        3 => {
            let s = key(&[(0.0, 0.82), (0.6, 1.12), (1.0, 1.0)], steps(p, 4.0));
            x.sx = s;
            x.sy = s;
        }
        4 => x.flash = key(&[(0.0, 0.0), (0.25, 0.6), (0.5, -0.2), (0.75, 0.6), (1.0, 0.0)], steps(p, 4.0)),
        5 => x.dy = key(&[(0.0, 0.0), (0.3, -12.0), (0.5, 0.0), (0.7, -5.0), (1.0, 0.0)], steps(p, 6.0)),
        6 => {
            let q = steps(p, 6.0);
            x.sx = key(&[(0.0, 1.0), (0.3, 1.12), (0.5, 0.9), (0.7, 1.05), (1.0, 1.0)], q);
            x.sy = key(&[(0.0, 1.0), (0.3, 0.88), (0.5, 1.1), (0.7, 0.95), (1.0, 1.0)], q);
        }
        7 => x.dx = key(&[(0.0, 0.0), (0.2, -2.0), (0.4, 2.0), (0.6, -1.5), (0.8, 1.5), (1.0, 0.0)], steps(p, 6.0)),
        8 => x.dx = key(&[(0.0, 0.0), (0.2, -6.0), (0.4, 6.0), (0.6, -4.0), (0.8, 4.0), (1.0, 0.0)], steps(p, 6.0)),
        9 => {
            let s = key(&[(0.0, 1.0), (0.15, 1.1), (0.3, 1.0), (0.45, 1.14), (0.7, 1.0), (1.0, 1.0)], steps(p, 6.0));
            x.sx = s;
            x.sy = s;
        }
        10 => x.dy = key(&[(0.0, 0.0), (0.5, -3.0), (1.0, 0.0)], steps(p, 8.0)),
        11 => x.ring = key(&[(0.0, 0.0), (0.5, 10.0), (1.0, 0.0)], steps(p, 4.0)),
        12 => x.hue = steps(p, 8.0),
        13 => x.scan = Some(steps(p, 8.0)),
        14 => x.fill = steps(p, 8.0),
        15 => x.onair = steps(p, 2.0) < 1.0,
        _ => return None,
    }
    Some(x)
}

/// A colour turned around the hue wheel (the rainbow animation).
pub fn hue_shift(c: egui::Color32, turn: f32) -> egui::Color32 {
    let (r, g, b) = (c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0);
    let a = turn * std::f32::consts::TAU;
    let (cs, sn) = (a.cos(), a.sin());
    let m = [
        [0.213 + cs * 0.787 - sn * 0.213, 0.715 - cs * 0.715 - sn * 0.715, 0.072 - cs * 0.072 + sn * 0.928],
        [0.213 - cs * 0.213 + sn * 0.143, 0.715 + cs * 0.285 + sn * 0.140, 0.072 - cs * 0.072 - sn * 0.283],
        [0.213 - cs * 0.213 - sn * 0.787, 0.715 - cs * 0.715 + sn * 0.715, 0.072 + cs * 0.928 + sn * 0.072],
    ];
    let o = |row: [f32; 3]| ((r * row[0] + g * row[1] + b * row[2]).clamp(0.0, 1.0) * 255.0) as u8;
    egui::Color32::from_rgb(o(m[0]), o(m[1]), o(m[2]))
}
