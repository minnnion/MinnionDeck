// ===== Plugins, crew and boards =====
// Minnion colour for each installed plugin, set when plugins load
const PLUGIN_BODY = {};
// Installed plugins, filled at start from the plugins folder (see loadPlugins). A plugin that is not
// in this list is "Not installed", and buttons that need one say so instead of running.
const plugins = [];
const pluginOf = (step) => ACTION[step.type].plugin || null;
const pluginState = (name) => { const p = plugins.find((q) => q.name === name); return p ? p.state : "missing"; };
const STATE_WORDS = { dizzy: "stopped", waking: "still starting", missing: "not installed", off: "turned off" };
const tileOwner = (t) => { const s = t.steps.find((x) => pluginOf(x)); return s ? pluginOf(s) : null; };
const isDual = (t) => t.mode === "single" && !!ACTION[t.steps[0].type].dual;

const crew = [
  { name: "Bolt", body: "mint", tall: false, expr: "open", look: 0 },
  { name: "Pip", body: "sky", tall: false, expr: "open", look: 0 },
  { name: "Tink", body: "lilac", tall: true, expr: "open", look: 0 }
];
const SIZES = [[1, 1], [2, 1], [2, 2], [1, 2]];
// Preset button colours: the crew palette first, then more. Any other colour comes from the custom picker.
const COLORS = [["tile", "White"], ["mint", "Mint"], ["sky", "Sky"], ["lilac", "Lilac"], ["peach", "Peach"], ["sun", "Sun"], ["coral", "Coral"],
  ["#9ad94b", "Lime"], ["#3ec1d3", "Teal"], ["#5c6bc0", "Indigo"], ["#ff8fab", "Pink"], ["#f4a259", "Orange"], ["#e63946", "Red"], ["#8d6e63", "Brown"], ["#b0bec5", "Grey"], ["#2d3142", "Charcoal"]];
const recentColors = [];
// A background colour as CSS: a #hex, or a palette token as var(--token). Anything else (an
// imported design can carry any text here, and this is interpolated into style attributes) falls
// back to the default background instead of reaching the markup.
const bgCss = (bg) => (typeof bg === "string" && /^#[0-9a-f]{3,8}$/i.test(bg) ? bg : typeof bg === "string" && /^[a-z][a-z0-9-]{0,31}$/i.test(bg) ? `var(--${bg})` : "#2d3142");
const lumOf = (hex) => {
  const v = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255).map((c) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4));
  return 0.2126 * v[0] + 0.7152 * v[1] + 0.0722 * v[2];
};
const ratio = (a, b) => (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
// Text and icons on a button: dark ink or white, whichever reads better on that colour
function textOn(bg) {
  if (bg === "tile") return "var(--ink)";
  if (!bg.startsWith("#")) return "var(--on-accent)";
  const l = lumOf(bg);
  return ratio(l, lumOf("#1e2433")) >= ratio(l, 1) ? "#1e2433" : "#ffffff";
}
const contrastOn = (bg) => { const l = lumOf(bg); const t = textOn(bg); return ratio(l, t === "#ffffff" ? 1 : lumOf("#1e2433")); };
const look = (bg = "tile", extra = {}) => ({ bg, onBg: "mint", shape: "full", textPos: "bottom", ...extra });
const T = (id, label, glyphName, w, h, mode, steps, style, on) => normalizeTile({ id, label, glyph: glyphName, w, h, mode, steps, style: style || look(), on: !!on });
const S = (type, f = {}) => ({ type, f: { ...newStep(type).f, ...f } });
const boards = {
  Main: [
    T("mute", "Mute mic", "mic", 1, 1, "single", [S("mic")], look("tile", { onBg: "coral" }), true),
    T("scene", "Screenshot", "camera", 2, 1, "single", [S("key", { keys: "Win+Shift+S" })], look("tile", { onBg: "mint" })),
    T("notes", "Open notes", "folder", 1, 1, "single", [S("run", { path: "C:\\Windows\\notepad.exe" })]),
    T("next", "Next track", "note", 1, 1, "single", [S("key", { keys: "NextTrack" })], look("sky")),
    T("live", "Go live", "play", 2, 1, "sequence", [S("url", { url: "https://example.com" }), S("delay", { ms: "3000" }), S("text", { text: "We are live" })], look("peach")),
    T("vol", "Volume up", "bolt", 1, 1, "single", [S("vol", { direction: "Up", amount: "10" })])
  ],
  Stream: [
    T("brb", "Be right back", "folder", 2, 2, "single", [S("text", { text: "Be right back!" })], look("lilac")),
    T("clip", "Clip last 30s", "bolt", 1, 1, "single", [S("key", { keys: "Win+Alt+G" })]),
    T("like", "Play or pause", "note", 1, 1, "single", [S("key", { keys: "PlayPause" })], look("sky", { shape: "icon" }))
  ],
  Empty: []
};
const findTile = (id) => Object.values(boards).flat().find((t) => t.id === id);
// Example animations: Go live shows ON AIR until it is pressed again
Object.assign(findTile("live").anim, { type: "onair", repeat: "loop" });
Object.assign(findTile("mute").anim, { type: "pop" });
Object.assign(findTile("next").anim, { type: "bounce" });
Object.assign(findTile("like").anim, { type: "heart" });
// Example states: a mute key that changes icon, and a three-state stream key
Object.assign(findTile("mute").states[1], { glyph: "cross", text: "Mic muted" });
boards.Stream.push(Object.assign(T("status", "Record", "broadcast", 1, 1, "single", [S("key", { keys: "Win+Alt+R" })]), {
  states: [
    { name: "Off", text: "Off", glyph: "broadcast", bg: "tile" },
    { name: "Recording", text: "Recording", glyph: "broadcast", bg: "coral" },
    { name: "Paused", text: "Paused", glyph: "playpause", bg: "sun" }
  ]
}));
const boardOf = (id) => Object.keys(boards).find((k) => boards[k].some((t) => t.id === id));

let current = "Main";
let editMode = false;
let busy = false;
let nextRunner = 0;
let selectedId = "live";

const boardEl = $("board"), grid = $("grid"), bench = $("bench"), toast = $("toast"), emptyEl = $("empty");
let sayTimer = 0;
const say = (msg) => {
  clearTimeout(sayTimer);
  sayTimer = setTimeout(() => { toast.innerHTML = ""; }, 3600);
  toast.innerHTML = "";
  const s = document.createElement("span");
  s.textContent = msg;
  toast.appendChild(s);
};
$("logo").innerHTML = minnion({ c: pal("mint"), expr: "happy", arm: true, scale: 4 });

// Example designs on buttons: a Be right back key on Main, and On air on the Stream key's Live state
boards.Main.push(Object.assign(T("brb-key", "Be right back", "hourglass", 1, 1, "single", [S("text", { text: "Be right back" })]), {}));
Object.assign(findTile("brb-key").states[0], { bgType: "design", design: "d-sunset" });
Object.assign(findTile("brb-key").anim, { type: "jelly" });

// Example GIFs and widgets
findTile("next").states[0].gif = { id: "eq", place: "icon", speed: "normal" };
findTile("live").states[0].gif = { id: "uptime", place: "bg", speed: "normal" };
Object.assign(findTile("status").states[1], { bgType: "design", design: "d-onair" });
findTile("brb").states[0].gif = { id: "countdown", place: "bg", speed: "normal" };

