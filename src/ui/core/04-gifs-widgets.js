// ===== GIFs and live widgets =====
// GIFs are pixel animations drawn as a strip of frames; the strip slides one frame at a time like a GIF.
// Widgets show live info, like home screen widgets on a phone.
function pixRng(seed) { let x = (seed >>> 0) || 1; return () => (x = (x * 1664525 + 1013904223) >>> 0) / 4294967296; }
const seeded = (seed, n, fn) => { const r = pixRng(seed); return Array.from({ length: n }, (_, i) => fn(r, i)); };
const RAIN = seeded(42, 9, (r) => [Math.floor(r() * 16), Math.floor(r() * 14)]);
const SNOW = seeded(7, 12, (r) => [Math.floor(r() * 16), Math.floor(r() * 12)]);
const STARS = seeded(11, 16, (r) => [Math.floor(r() * 16), Math.floor(r() * 12)]);
const CODE = seeded(5, 8, (r, i) => [i * 2, Math.floor(r() * 16)]);
const DISCO = ["#ff5a5f", "#ffc93c", "#5fd0a8", "#7cc4ff", "#b79cff"];
const ECG = [0, 0, 0, 0, 0, -1, 0, 1, -5, 4, 0, 0, 0, -1, -1, 0];
const SPIN = [[7, 1], [10, 2], [11, 5], [10, 8], [7, 9], [4, 8], [3, 5], [4, 2]];
const HEART = [[0, 0], [2, 0], [0, 1], [1, 1], [2, 1], [1, 2]];
const GIFS = [
  { id: "fire", name: "Fire", desc: "Flickering flames", n: 8, ms: 110, gen: (f) => {
    const px = [], r = pixRng(f * 977 + 3);
    for (let x = 0; x < 16; x++) {
      const h = Math.max(1, Math.round(3 + (3 - Math.abs(x - 7.5) / 3) + r() * 4));
      for (let k = 0; k < h; k++) px.push([x, 11 - k, k / h < 0.4 ? "#c93c41" : k / h < 0.75 ? "#ff8a3c" : "#ffc93c"]);
      if (r() < 0.25) px.push([x, 10 - h - Math.floor(r() * 2), "#fff3b0"]);
    }
    return px;
  } },
  { id: "rain", name: "Rain", desc: "Falling drops", n: 7, ms: 90, gen: (f) => RAIN.flatMap(([x, y0]) => { const y = ((y0 + f * 2) % 14) - 1; return [[x, y, "#7cc4ff"], [x, y + 1, "#4c97d6"]]; }) },
  { id: "snow", name: "Snow", desc: "Slow snowfall at night", n: 12, ms: 160, bg: "#2d3142", gen: (f) => SNOW.map(([x0, y0]) => [(x0 + (Math.floor((f + y0) / 3) % 2)) % 16, (y0 + f) % 12, "#ffffff"]) },
  { id: "stars", name: "Stars", desc: "A twinkling night sky", n: 6, ms: 220, bg: "#1c2232", gen: (f) => {
    const px = STARS.map(([x, y], i) => [x, y, (i + f) % 3 === 0 ? "#5c6bc0" : "#ffffff"]);
    if (f % 2 === 0) [[11, 3], [10, 3], [12, 3], [11, 2], [11, 4]].forEach(([x, y]) => px.push([x, y, "#ffc93c"]));
    return px;
  } },
  { id: "eq", name: "Equaliser", desc: "Bouncing music bars", n: 8, ms: 100, gen: (f) => {
    const px = [];
    for (let i = 0; i < 5; i++) {
      const h = 2 + Math.round((Math.sin((f / 8) * 2 * Math.PI + i * 1.3) + 1) * 4);
      for (let k = 0; k < h; k++) for (let dx = 0; dx < 2; dx++) px.push([1 + i * 3 + dx, 11 - k, k === h - 1 ? "#ffc93c" : "#3aa585"]);
    }
    return px;
  } },
  { id: "ecg", name: "Heart line", desc: "A pulse trace", n: 16, ms: 70, gen: (f) => {
    const px = []; let prev = null;
    for (let x = 0; x < 16; x++) {
      const y = 6 + ECG[(x + f) % 16];
      if (prev !== null) for (let yy = Math.min(prev, y); yy <= Math.max(prev, y); yy++) px.push([x, yy, "#ff5a5f"]);
      else px.push([x, y, "#ff5a5f"]);
      prev = y;
    }
    return px;
  } },
  { id: "waves", name: "Waves", desc: "Rolling water", n: 8, ms: 130, gen: (f) => {
    const px = [];
    for (let x = 0; x < 16; x++) {
      const top = 6 + Math.round(Math.sin(((x + f * 2) / 16) * 4 * Math.PI) * 1.5);
      for (let y = top; y < 12; y++) px.push([x, y, y === top ? "#ffffff" : y < top + 2 ? "#7cc4ff" : "#4c97d6"]);
    }
    return px;
  } },
  { id: "matrix", name: "Code rain", desc: "Green code falling", n: 16, ms: 80, bg: "#0b1a10", gen: (f) => CODE.flatMap(([x, y0]) => { const y = ((y0 + f) % 16) - 3; return [[x, y, "#b8ffcb"], [x, y - 1, "#3aa585"], [x, y - 2, "#1f6f4a"], [x, y - 3, "#1f6f4a"]]; }) },
  { id: "disco", name: "Disco", desc: "Flashing colour squares", n: 5, ms: 180, gen: (f) => {
    const px = [];
    for (let bx = 0; bx < 4; bx++) for (let by = 0; by < 3; by++) for (let dx = 0; dx < 3; dx++) for (let dy = 0; dy < 3; dy++) px.push([bx * 4 + dx, by * 4 + dy, DISCO[(bx + by + f) % 5]]);
    return px;
  } },
  { id: "spinner", name: "Loading", desc: "A spinning wheel of dots", n: 8, ms: 90, gen: (f) => SPIN.flatMap(([x, y], i) => { const c = i === f ? "#5c6bc0" : i === (f + 7) % 8 ? "#9fa8da" : "#cfd8dc"; return [[x, y, c], [x + 1, y, c], [x, y + 1, c], [x + 1, y + 1, c]]; }) },
  { id: "radar", name: "Radar", desc: "A sweeping scan", n: 8, ms: 140, bg: "#10261c", gen: (f) => {
    const px = [];
    for (let a = 0; a < 32; a++) px.push([Math.round(8 + Math.cos((a / 32) * 2 * Math.PI) * 5), Math.round(6 + Math.sin((a / 32) * 2 * Math.PI) * 5), "#1f6f4a"]);
    const line = (ang, c) => { for (let r = 0; r <= 5; r++) px.push([Math.round(8 + Math.cos(ang) * r), Math.round(6 + Math.sin(ang) * r), c]); };
    line(((f - 1) / 8) * 2 * Math.PI, "#2e7d5b");
    line((f / 8) * 2 * Math.PI, "#5fd0a8");
    if (f === 7 || f === 0) px.push([11, 3, "#b8ffcb"], [12, 3, "#b8ffcb"]);
    return px;
  } },
  { id: "hearts", name: "Hearts", desc: "Little hearts floating up", n: 8, ms: 120, gen: (f) => [2, 7, 12].flatMap((x0, i) => { const y = 11 - ((f * 2 + i * 5) % 16); return HEART.map(([dx, dy]) => [x0 + dx - 1, y + dy, "#ff8fab"]); }) },
  { id: "minnion", name: "Waving minnion", desc: "A crew member says hi", n: 4, ms: 220, w: 16, h: 16, gen: (f) => {
    const svg = minnion({ c: { k: "#1e2433", p: "#1e2433", w: "#ffffff", m: "#5fd0a8", M: "#3aa585", r: "#ff5a5f" }, expr: f === 3 ? "sleepy" : "happy", arm: f % 2 === 0, scale: 1 });
    return [...svg.matchAll(/x="(\d+)" y="(\d+)" width="1" height="1" style="fill:([^"]+)"/g)].map((m) => [Number(m[1]), Number(m[2]), m[3]]);
  } }
].map((g) => ({ kind: "gif", group: "GIFs", w: 16, h: 12, ...g }));
const WIDGETS = [
  { id: "clock", name: "Clock", desc: "The time, live" },
  { id: "date", name: "Date", desc: "Today's date" },
  { id: "uptime", name: "Stream uptime", desc: "How long you have been live" },
  { id: "viewers", name: "Viewers", desc: "Live viewer count" },
  { id: "countdown", name: "Countdown", desc: "Back in five minutes" },
  { id: "nowplaying", name: "Now playing", desc: "Song title and bars" },
  { id: "weather", name: "Weather", desc: "Sky and temperature" },
  { id: "cpu", name: "CPU", desc: "How busy the PC is" },
  { id: "battery", name: "Phone battery", desc: "Battery of a paired phone" }
].map((w) => ({ kind: "widget", group: "Widgets", ...w }));
const GIF = Object.fromEntries([...GIFS, ...WIDGETS].map((g) => [g.id, g]));
const GIF_NONE = () => ({ id: "none", place: "bg", speed: "normal" });
const gifOf = (st) => (st && st.gif && GIF[st.gif.id]) || null;
// A GIF's size and frame count as numbers. Drawn and imported GIFs come from files, so these are
// coerced before they reach the SVG (and the frame count is capped, so a file can't make it loop
// forever).
const gifDims = (g) => ({ w: Math.max(1, cssNum(g.w, 16)), h: Math.max(1, cssNum(g.h, 16)), n: Math.min(Math.max(0, Math.floor(cssNum(g.n, 0))), 1000) });
// Each GIF's drawn picture, made once. Kept here rather than on the GIF, so a GIF read from a file
// can't bring a ready-made picture of its own.
const GIF_SVG = new WeakMap();
function stripSVG(g) {
  const kept = GIF_SVG.get(g);
  if (kept !== undefined) return kept;
  const { w, h, n } = gifDims(g);
  const bg = cssHex(g.bg);
  let out = bg ? `<rect width="${w * n}" height="${h}" fill="${bg}"/>` : "";
  for (let f = 0; f < n; f++) {
    const byC = {};
    (typeof g.gen === "function" ? g.gen(f) : []).forEach(([x, y, c]) => { if (!(x >= 0 && y >= 0 && x < w && y < h)) return; (byC[c] = byC[c] || []).push(`M${x + f * w} ${y}h1v1h-1z`); });
    for (const c in byC) out += `<path fill="${htmlAttr(c)}" d="${byC[c].join("")}"/>`;
  }
  GIF_SVG.set(g, out);
  return out;
}
const gifStrip = (g, speed = "normal") => {
  if (g.src) return `<img class="gif-file" src="${htmlAttr(g.src)}" alt="" draggable="false">`;
  const { w, h, n } = gifDims(g);
  return `<svg class="gif-strip" viewBox="0 0 ${w * n} ${h}" preserveAspectRatio="none" shape-rendering="crispEdges" aria-hidden="true" style="width:${n * 100}%;animation-duration:${Math.round(cssNum(g.ms, 125) * n * (SPEED[speed] || 1))}ms;animation-timing-function:steps(${n}, end)">${stripSVG(g)}</svg>`;
};
const gifBox = (g, speed, size) => `<span class="gif-ico" style="width:${Math.round((size * g.w) / g.h)}px;height:${size}px">${gifStrip(g, speed)}</span>`;
const WSTART = Date.now();
let wViewers = 1284, wCpu = 23;
const p2 = (n) => String(n).padStart(2, "0");
function widgetFace(id) {
  const d = new Date();
  const secs = Math.floor((Date.now() - WSTART) / 1000);
  switch (id) {
    case "clock": return `<b>${p2(d.getHours())}:${p2(d.getMinutes())}</b><small>${["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"][d.getDay()]}</small>`;
    case "date": return `<small>${d.toLocaleString("en", { month: "short" }).toUpperCase()}</small><b>${d.getDate()}</b>`;
    case "uptime": { const s2 = secs + 754; return `<small class="live">● LIVE</small><b>${p2(Math.floor(s2 / 3600))}:${p2(Math.floor(s2 / 60) % 60)}:${p2(s2 % 60)}</b>`; }
    case "viewers": return `<small>VIEWERS</small><b>${wViewers.toLocaleString("en")}</b>`;
    case "countdown": { const left = 300 - (secs % 300); return `<small>BACK IN</small><b>${Math.floor(left / 60)}:${p2(left % 60)}</b>`; }
    case "nowplaying": return `<span class="mq"><span>Pixel Dreams · The Crew</span></span><span class="eqm"><i></i><i></i><i></i><i></i></span>`;
    case "weather": return `${glyph("sun", 16)}<b>29°</b><small>SUNNY</small>`;
    case "cpu": return `<small>CPU</small><b>${wCpu}%</b><i class="wbar"><i style="width:${wCpu}%"></i></i>`;
    case "battery": return `<small>PIXEL 8</small><b>76%</b><i class="wbar"><i style="width:76%"></i></i>`;
    default: return "";
  }
}
// Widgets tick once a second wherever they show
setInterval(() => {
  wViewers = Math.max(900, wViewers + Math.round((Math.random() - 0.45) * 18));
  wCpu = Math.max(8, Math.min(64, wCpu + Math.round((Math.random() - 0.5) * 10)));
  document.querySelectorAll('.wdg[data-w]:not([data-w="nowplaying"])').forEach((e) => { e.innerHTML = widgetFace(e.dataset.w); });
}, 1000);
// What a state shows in the icon spot, behind the button, or as its face
function gifParts(st, size) {
  const g = gifOf(st);
  const out = { icon: glyph(st.glyph, size), bg: "", wdg: "", kind: g ? g.kind : null };
  if (!g) return out;
  if (g.kind === "widget") { out.icon = ""; out.wdg = `<span class="wdg" data-w="${g.id}" aria-hidden="true">${widgetFace(g.id)}</span>`; }
  else if (st.gif.place === "icon") out.icon = gifBox(g, st.gif.speed, size);
  else out.bg = `<span class="gif-bg" aria-hidden="true">${gifStrip(g, st.gif.speed)}</span>`;
  return out;
}

