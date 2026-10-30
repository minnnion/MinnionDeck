// ===== Values from saved or imported files, made safe for markup =====
// Profiles, designs and GIFs can come from shared files, so every such value that reaches a style
// attribute, an SVG attribute or a class name goes through one of these first: a hostile file can't
// break out of the markup (see docs/security-review-2026-10-30.md). Colours are a hex or a palette
// token; numbers are coerced (a non-number becomes the fallback, never raw text); idents are
// letters, digits and dashes. Defined early so the page scripts after this one can use them.
const cssNum = (v, fb) => { const n = Number(v); return Number.isFinite(n) ? n : fb; };
const cssHex = (v, fb = null) => (typeof v === "string" && /^#[0-9a-f]{3,8}$/i.test(v) ? v : fb);
const cssColour = (v, fb = "currentColor") => cssHex(v) || (typeof v === "string" && /^[a-z][a-z0-9-]{0,31}$/i.test(v) ? `var(--${v})` : fb);
const cssIdent = (v, fb = "none") => (typeof v === "string" && /^[a-z0-9-]{1,32}$/i.test(v) ? v : fb);
// Any text placed inside a quoted attribute (a URL, an SVG fill): it can't close the quotes
const htmlAttr = (v) => String(v).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);

// ===== The minnion: an original 16px-wide pixel character =====
// k outline, m body, M body shade, w eye white, p pupil and mouth, r antenna bulb.
const MINNION_BODY = [
  ".......rr.......",
  ".......rr.......",
  "........k.......",
  ".....kkkkkk.....",
  "...kkmmmmmmkk...",
  "..kmmmmmmmmmmk..",
  "..kmmmmmmmmmmk..",
  ".kmmmmmmmmmmmmk.",
  ".kmmmmmmmmmmmmk.",
  ".kmmmmmmmmmmmmk.",
  ".kmmmmmmmmmmmmk.",
  ".kMmmmmmmmmmmMk.",
  "..kMMmmmmmmMMk..",
  "...kkkkkkkkkk..."
];
const MINNION_FEET = { idle: "...kk......kk...", a: "..kk.......kk...", b: "....kk....kk...." };
const BLANK = "................";

// expr: open, happy, surprised, sleepy, dizzy. look: -1 left, 0 ahead, 1 right.
// pose: squash (one pixel shorter) or stretch (one pixel taller). Height stays constant.
function minnion({ c, expr = "open", look = 0, arm = false, feet = "idle", tall = false, pose = "", scale = 3 }) {
  const rows = MINNION_BODY.map((r) => [...r]);
  if (tall) rows.splice(9, 0, [...rows[9]], [...rows[9]]);
  rows.push([...MINNION_FEET[feet]], [...BLANK]);
  const set = (x, y, ch) => { if (rows[y] && rows[y][x] !== undefined) rows[y][x] = ch; };
  for (const ex of [4, 9]) {
    if (expr === "open" || expr === "surprised") {
      for (let y = 6; y <= 8; y++) for (let x = ex; x < ex + 3; x++) set(x, y, "w");
      const px = ex + 1 + look;
      set(px, 7, "p");
      if (expr === "open") set(px, 8, "p");
    } else if (expr === "happy") {
      set(ex, 7, "p"); set(ex + 1, 6, "p"); set(ex + 2, 7, "p");
    } else if (expr === "sleepy") {
      for (let x = ex; x < ex + 3; x++) set(x, 7, "p");
    } else if (expr === "dizzy") {
      set(ex, 6, "p"); set(ex + 2, 6, "p"); set(ex + 1, 7, "p"); set(ex, 8, "p"); set(ex + 2, 8, "p");
    }
  }
  const my = tall ? 12 : 10;
  const mouth = {
    happy: [[6, 0], [9, 0], [7, 1], [8, 1]],
    surprised: [[7, 0], [8, 0], [7, 1], [8, 1]],
    dizzy: [[6, 0], [7, 1], [8, 0], [9, 1]]
  }[expr] || [[7, 0], [8, 0]];
  mouth.forEach(([x, dy]) => set(x, my + dy, "p"));
  if (arm) [4, 5, 6].forEach((y) => set(0, y, "k"));
  if (pose === "squash") { rows.splice(9, 1); rows.unshift([...BLANK]); }
  if (pose === "stretch") { rows.splice(9, 0, [...rows[9]]); rows.pop(); }
  let rects = "";
  rows.forEach((row, y) => row.forEach((ch, x) => {
    if (c[ch]) rects += `<rect x="${x}" y="${y}" width="1" height="1" style="fill:${c[ch]}"/>`;
  }));
  const h = rows.length;
  return `<svg class="sprite" viewBox="0 0 16 ${h}" width="${16 * scale}" height="${h * scale}" shape-rendering="crispEdges" aria-hidden="true">${rects}</svg>`;
}
const BODIES = {
  mint: ["var(--mint)", "var(--mint-dark)"],
  sky: ["var(--sky)", "var(--sky-dark)"],
  lilac: ["var(--lilac)", "var(--lilac-dark)"],
  peach: ["var(--peach)", "var(--peach-dark)"],
  coral: ["var(--coral)", "var(--coral-dark)"]
};
// An unknown body colour (say, from a plugin) falls back to mint instead of breaking the drawing
const pal = (body, bulb = "var(--coral)") => { const b = BODIES[body] || BODIES.mint; return { k: "var(--ink)", p: "var(--eye)", w: "#ffffff", m: b[0], M: b[1], r: bulb }; };

// 8x8 tile glyphs, drawn in ink
const GLYPHS = {
  play: ["........", "##......", "####....", "######..", "######..", "####....", "##......", "........"],
  bolt: ["....###.", "...##...", "..##....", ".#######", "....##..", "...##...", "..##....", ".##....."],
  note: ["....####", "....#..#", "....#..#", "....#..#", "..###.##", ".####.##", ".###....", "........"],
  folder: ["........", "##......", "########", "#......#", "#......#", "#......#", "########", "........"],
  mic: ["...##...", "..####..", "..####..", "..####..", ".#.##.#.", ".#....#.", "..####..", "...##..."],
  chat: ["........", "########", "#......#", "#.#.#..#", "#......#", "########", ".##.....", "#......."]
};
// Interface-only glyphs, kept out of the tile icon picker
const UI_GLYPHS = {
  moon: ["..###...", ".##.....", "##......", "##......", "##......", "##....#.", ".##..##.", "..####.."],
  sun: ["...##...", ".#....#.", "..####..", "#.####.#", "#.####.#", "..####..", ".#....#.", "...##..."],
  camera: ["........", "..##....", "########", "#..##..#", "#.#..#.#", "#.#..#.#", "#..##..#", "########"]
};
// Action icons. In the app each one is a file in its plugin's folder: plugins/<plugin>/icons/<action>.png
const ACTION_GLYPHS = {
  keyboard: ["........", "########", "#.#.#.##", "########", "##.#.#.#", "########", "##....##", "########"],
  type: ["########", "#..##..#", "...##...", "...##...", "...##...", "...##...", "..####..", "........"],
  mouse: ["..####..", ".#.#..#.", ".#.#..#.", ".######.", ".#....#.", ".#....#.", "..####..", "........"],
  app: ["########", "#.#.####", "########", "#......#", "#.#....#", "#..#...#", "#.#.##.#", "########"],
  globe: ["..####..", ".#.##.#.", "#..##..#", "########", "#..##..#", ".#.##.#.", "..####..", "........"],
  speaker: ["...#..#.", "..##...#", "####.#.#", "####.#.#", "####.#.#", "..##...#", "...#..#.", "........"],
  hourglass: ["########", ".#....#.", "..#..#..", "...##...", "...##...", "..#..#..", ".#.##.#.", "########"],
  clapper: ["##..##..", "..##..##", "........", "########", "#......#", "#.####.#", "#......#", "########"],
  broadcast: ["........", ".#....#.", "#..##..#", "#.####.#", "#.####.#", "#..##..#", ".#....#.", "........"],
  rewind: ["........", "...#...#", "..##..##", ".###.###", ".###.###", "..##..##", "...#...#", "........"],
  playpause: ["........", "#...#.#.", "##..#.#.", "###.#.#.", "###.#.#.", "##..#.#.", "#...#.#.", "........"],
  skip: ["........", "#...#..#", "##..##.#", "###.####", "###.####", "##..##.#", "#...#..#", "........"],
  heart: ["........", ".##..##.", "########", "########", ".######.", "..####..", "...##...", "........"],
  flag: ["#.......", "######..", "#######.", "######..", "#.......", "#.......", "#.......", "#......."],
  headset: ["..####.#", ".#....#.", "#....#.#", "#...#..#", "##.#..##", "###...##", "##....##", "#......."],
  caret: ["........", "........", ".######.", "..####..", "...##...", "........", "........", "........"],
  monitor: ["########", "#......#", "#......#", "#......#", "########", "...##...", ".######.", "........"],
  star: ["...##...", "...##...", "########", ".######.", "..####..", ".##..##.", "##....##", "........"],
  home: ["...##...", "..####..", ".######.", "########", ".#....#.", ".#.##.#.", ".#.##.#.", ".######."],
  lock: ["..####..", ".#....#.", ".#....#.", "########", "###..###", "###..###", "########", "........"],
  camera: ["........", "..##....", "########", "#..##..#", "#.#..#.#", "#..##..#", "########", "........"],
  bulb: ["..####..", ".#....#.", "#......#", "#......#", ".#....#.", "..####..", "..####..", "...##..."],
  power: ["...##...", ".#.##.#.", "#..##..#", "#......#", "#......#", ".#....#.", "..####..", "........"],
  gear: ["...##...", ".######.", ".##..##.", "###..###", "###..###", ".##..##.", ".######.", "...##..."],
  check: ["........", ".......#", "......##", "#....##.", "##..##..", ".####...", "..##....", "........"],
  cross: ["##....##", "###..###", ".######.", "..####..", "..####..", ".######.", "###..###", "##....##"],
  up: ["...##...", "..####..", ".######.", "########", "...##...", "...##...", "...##...", "...##..."],
  down: ["...##...", "...##...", "...##...", "...##...", "########", ".######.", "..####..", "...##..."]
};
// Icons that come with plugins ("plug:<plugin>:<file>"), filled when plugins load. One whose
// plugin is gone shows as the bolt.
const PLUGIN_ICON_URL = {};
// The pixel icon set (00-pixel-icons.js), unpacked on first use: names in order, and each icon's
// SVG rectangles once drawn (one per run of filled pixels in a row)
let PX_ICONS = null;
const pxIcons = () => PX_ICONS || (PX_ICONS = new Map(PX_ICON_DATA.split(";").map((s) => s.split(":"))));
const PX_SVG = {};
const pxName = (id) => id.slice(3).replace(/-/g, " ").replace(/^./, (c) => c.toUpperCase());
// App and game logos (00-brand-icons.js), "si:<slug>": slug to [slug, name, brand colour, pixels]
let SI_MAP = null;
const siIcons = () => SI_MAP || (SI_MAP = new Map(SI_ICONS.map((x) => [x[0], x])));
const siOf = (id) => siIcons().get(id.slice(3)) || null;
// The most-used apps have a hand-made colour logo (00-brand-art.js, 16 × 16) that takes the place
// of the one-colour drawing, and a tile colour to sit on like the app's own icon
const artOf = (id) => (id.startsWith("si:") && BRAND_ART[id.slice(3)]) || null;
const ART_SVG = {};
function artRects(id) {
  if (ART_SVG[id] !== undefined) return ART_SVG[id];
  const a = artOf(id);
  if (!a) return (ART_SVG[id] = null);
  let r = "";
  // One rectangle per run of the same colour in a row
  a.p.forEach((row, y) => {
    for (let x = 0; x < row.length;) {
      let e = x + 1;
      while (e < row.length && row[e] === row[x]) e++;
      if (row[x] !== ".") r += `<rect x="${x}" y="${y}" width="${e - x}" height="1" fill="#${a.c[row[x]]}"/>`;
      x = e;
    }
  });
  return (ART_SVG[id] = r);
}
// The colour a logo sits on: its tile colour when it has a colour logo, else its brand colour
const siBg = (id) => { const a = artOf(id); if (a) return `#${a.bg}`; const s = siOf(id); return s ? `#${s[2].toLowerCase()}` : null; };
// The SVG rectangles of a 24 × 24 icon ("px:…" or "si:…"), once drawn
function pxRects(id) {
  if (PX_SVG[id] !== undefined) return PX_SVG[id];
  const b64 = id.startsWith("si:") ? (siOf(id) || [])[3] : pxIcons().get(id.slice(3));
  if (!b64) return (PX_SVG[id] = null);
  const bytes = atob(b64);
  let r = "";
  for (let y = 0; y < 24; y++) {
    let start = -1;
    for (let x = 0; x <= 24; x++) {
      const on = x < 24 && bytes.charCodeAt(y * 3 + (x >> 3)) & (0x80 >> (x & 7));
      if (on && start < 0) start = x;
      if (!on && start >= 0) { r += `<rect x="${start}" y="${y}" width="${x - start}" height="1"/>`; start = -1; }
    }
  }
  return (PX_SVG[id] = r);
}
const glyph = (name, size = 24) => {
  if (typeof name === "string" && (name.startsWith("px:") || name.startsWith("si:"))) {
    const art = artRects(name);
    if (art) return `<svg viewBox="0 0 16 16" width="${size}" height="${size}" shape-rendering="crispEdges" aria-hidden="true">${art}</svg>`;
    const r = pxRects(name);
    if (r !== null) return `<svg viewBox="0 0 24 24" width="${size}" height="${size}" shape-rendering="crispEdges" aria-hidden="true" style="fill:currentColor">${r}</svg>`;
    name = "bolt";
  }
  if (typeof name === "string" && name.startsWith("plug:")) {
    const url = PLUGIN_ICON_URL[name];
    if (url) return `<img class="glyph-img" src="${url}" width="${size}" height="${size}" alt="" aria-hidden="true" draggable="false">`;
    name = "bolt";
  }
  let r = "";
  (GLYPHS[name] || UI_GLYPHS[name] || ACTION_GLYPHS[name] || GLYPHS.bolt).forEach((row, y) => [...row].forEach((c, x) => { if (c === "#") r += `<rect x="${x}" y="${y}" width="1" height="1"/>`; }));
  return `<svg viewBox="0 0 8 8" width="${size}" height="${size}" shape-rendering="crispEdges" aria-hidden="true" style="fill:currentColor">${r}</svg>`;
};

const osReduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
// Settings > Appearance > Animations. "reduced": the crew stops walking, small effects stay.
// "off": nothing moves. Windows' own reduce-motion setting always counts as "off".
let motionLevel = "full";
let reduced = osReduced;
const noMotion = () => osReduced || motionLevel === "off";
// Dropdowns fold open (CSS dropIn) and fold shut before they hide; at once when motion is off.
// Opening again while one is still folding shut stops the fold.
function showPop(e) {
  if (!e) return;
  clearTimeout(e._popT);
  e.classList.remove("pop-out");
  e.hidden = false;
}
function hidePop(e) {
  if (!e || e.hidden) return;
  clearTimeout(e._popT);
  if (noMotion()) { e.classList.remove("pop-out"); e.hidden = true; return; }
  e.classList.add("pop-out");
  e._popT = setTimeout(() => { e.classList.remove("pop-out"); e.hidden = true; }, 120);
}
function setMotion(level) {
  motionLevel = level === "reduced" || level === "off" ? level : "full";
  reduced = osReduced || motionLevel !== "full";
  document.documentElement.dataset.motion = noMotion() ? "off" : motionLevel;
}
const wait = (ms) => new Promise((r) => setTimeout(r, reduced ? 0 : ms));
const $ = (id) => document.getElementById(id);
const clone = (o) => JSON.parse(JSON.stringify(o));

