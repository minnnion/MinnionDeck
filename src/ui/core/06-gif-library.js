// ===== Your own GIFs, drawn in the GIF maker (it was in the app; it is on the website now) =====
// Each GIF is a list of frames; a frame is one colour (or nothing) per pixel. They join every GIF list in the app.
const USER_GIFS = [];
let ugSeq = 0;
function makeUserGif(name, size, frames, fps = 8) {
  const g = {
    id: `g${++ugSeq}`, name, desc: "Made in the GIF maker", kind: "gif", group: "GIFs", w: size, h: size, n: frames.length, ms: Math.round(1000 / fps), fps, frames, user: true,
    gen(f) { const px = [], fr = this.frames[f] || []; for (let i = 0; i < fr.length; i++) if (fr[i]) px.push([i % this.w, Math.floor(i / this.w), fr[i]]); return px; }
  };
  USER_GIFS.push(g); GIFS.push(g); GIF[g.id] = g;
  return g;
}
function gifTouched(g) { g.n = g.frames.length; g.ms = Math.round(1000 / g.fps); GIF_SVG.delete(g); }
const blankFrame = (s) => new Array(s * s).fill(null);
// Example GIFs, as if drawn by hand
(() => {
  const S = 16, frames = [];
  for (let f = 0; f < 8; f++) {
    const fr = blankFrame(S), t = Math.abs(Math.sin((f / 8) * Math.PI));
    const ground = f === 0;
    const y = Math.round(11 - t * 9), w = ground ? 6 : 4, h = ground ? 3 : 4, x0 = 8 - w / 2;
    for (let yy = 0; yy < h; yy++) for (let xx = 0; xx < w; xx++) fr[(y + (ground ? 1 : 0) + yy) * S + x0 + xx] = yy === 0 && xx === 1 ? "#ffd2d4" : yy === h - 1 ? "#c93c41" : "#ff5a5f";
    const sw = Math.max(2, 6 - Math.round(t * 4));
    for (let xx = 0; xx < sw; xx++) fr[15 * S + 8 - sw / 2 + xx] = "#b0bec5";
    frames.push(fr);
  }
  makeUserGif("Bouncing ball", S, frames, 10);
})();
(() => {
  const S = 16, frames = [];
  [0.92, 1.08, 0.92, 0.84].forEach((k) => {
    const fr = blankFrame(S);
    for (let y = 0; y < S; y++) for (let x = 0; x < S; x++) {
      const u = (x - 7.5) / (5.2 * k), v = -(y - 7.2) / (5.2 * k);
      const a = u * u + v * v - 1;
      if (a * a * a - u * u * v * v * v <= 0) fr[y * S + x] = x < 7 && y < 6 ? "#ffd2d4" : y > 10 ? "#c93c41" : "#ff5a5f";
    }
    frames.push(fr);
  });
  makeUserGif("Heartbeat", S, frames, 6);
})();
const M_LOGO = ["................", "................", "................", ".##.........##..", ".###.......###..", ".####.....####..", ".##.##...##.##..", ".##..##.##..##..", ".##...###...##..", ".##....#....##..", ".##.........##..", ".##.........##..", ".##.........##..", "................", "................", "................"];
(() => {
  const S = 16, frames = [];
  for (let f = 0; f < 8; f++) {
    const fr = blankFrame(S);
    M_LOGO.forEach((row, y) => [...row].forEach((c, x) => { if (c !== "#") return; const d = x + y - (f * 4 - 4); fr[y * S + x] = d >= 0 && d <= 2 ? "#ffffff" : d === 3 ? "#b8ffcb" : "#5fd0a8"; }));
    frames.push(fr);
  }
  makeUserGif("M logo shine", S, frames, 8);
})();

// ===== More GIFs: packs of ready-drawn GIFs, and GIF files from your computer =====
const IMPORTED_GIFS = [];
function uploadGif(after) {
  const inp = document.createElement("input");
  inp.type = "file";
  inp.accept = "image/gif, image/webp, image/png";
  inp.addEventListener("change", () => {
    const f = inp.files && inp.files[0];
    if (!f) return;
    const rd = new FileReader();
    rd.onload = () => {
      const img = new Image();
      img.onload = () => {
        const g = { id: `f${IMPORTED_GIFS.length + 1}${Date.now().toString(36)}`, name: f.name.replace(/\.[^.]+$/, "").slice(0, 24), desc: "Imported GIF file", kind: "gif", group: "GIFs", file: true, src: rd.result, w: img.naturalWidth || 16, h: img.naturalHeight || 16, n: 1, ms: 100 };
        IMPORTED_GIFS.push(g); GIFS.push(g); GIF[g.id] = g;
        after(g);
      };
      img.src = rd.result;
    };
    rd.readAsDataURL(f);
  });
  inp.click();
}
// Scale an 8 × 8 glyph up to a 16 × 16 frame
function glyphFrame(name, colour, dx = 0, dy = 0) {
  const fr = blankFrame(16), rows = GLYPHS[name] || UI_GLYPHS[name] || ACTION_GLYPHS[name];
  rows.forEach((row, y) => [...row].forEach((c, x) => { if (c !== "#") return; for (let a = 0; a < 2; a++) for (let b = 0; b < 2; b++) { const xx = x * 2 + a + dx, yy = y * 2 + b + dy; if (xx >= 0 && yy >= 0 && xx < 16 && yy < 16) fr[yy * 16 + xx] = colour; } }));
  return fr;
}
const GIF_PACKS = [
  { id: "gaming", name: "Gaming", desc: "Coin spin, Level up, Power star", make: () => {
    const coin = [];
    for (let f = 0; f < 8; f++) {
      const fr = blankFrame(16), hw = Math.max(1, Math.round(6 * Math.abs(Math.cos((f / 8) * Math.PI))));
      for (let y = 2; y < 14; y++) { const dy = (y - 7.5) / 6, half = Math.round(hw * Math.sqrt(Math.max(0, 1 - dy * dy))); for (let x = 8 - half - 1; x <= 8 + half; x++) if (x >= 0 && x < 16) fr[y * 16 + x] = x === 8 - half - 1 || x === 8 + half || y === 2 || y === 13 ? "#c98a1a" : hw > 2 && x === 8 - Math.ceil(half / 2) ? "#fff3b0" : "#ffc93c"; }
      coin.push(fr);
    }
    const up = [];
    for (let f = 0; f < 8; f++) { const fr = glyphFrame("up", "#5fd0a8", 0, 16 - ((f * 4) % 32)); for (let k = 0; k < 2; k++) { const y = (f * 4 + 6 + k * 8) % 16; fr[y * 16 + 2] = "#ffffff"; fr[((y + 8) % 16) * 16 + 13] = "#ffc93c"; } up.push(fr); }
    const star = ["#ffc93c", "#ffffff", "#ff8a3c", "#ffc93c", "#fff3b0", "#b79cff"].map((c, f) => { const fr = glyphFrame("star", c); if (f % 2) [[0, 0], [15, 0], [0, 15], [15, 15]].forEach(([x, y]) => { fr[y * 16 + x] = "#ffffff"; }); return fr; });
    return [makeUserGif("Coin spin", 16, coin, 10), makeUserGif("Level up", 16, up, 10), makeUserGif("Power star", 16, star, 6)].map((g) => Object.assign(g, { desc: "From the Gaming pack" }));
  } },
  { id: "stream", name: "Streaming", desc: "Rec blink, Sound waves, Typing dots", make: () => {
    const rec = [0, 1, 2, 3].map((f) => { const fr = blankFrame(16); for (let y = 0; y < 16; y++) for (let x = 0; x < 16; x++) { const d = Math.hypot(x - 7.5, y - 7.5); if (d <= 5) fr[y * 16 + x] = f < 2 ? (d < 2 && f === 0 ? "#ff8a8d" : "#ff5a5f") : "#7a1f22"; else if (f === 0 && d <= 7 && d > 6) fr[y * 16 + x] = "#ffd2d4"; } return fr; });
    const waves = [0, 1, 2, 3, 4, 5].map((f) => { const fr = glyphFrame("speaker", "#e4eaf6", -4, 0); const arcs = f % 4; for (let a = 1; a <= arcs; a++) { const r = 3 + a * 3; for (let t = -40; t <= 40; t += 8) { const x = Math.round(5 + Math.cos((t * Math.PI) / 180) * r), y = Math.round(8 + Math.sin((t * Math.PI) / 180) * r); if (x >= 0 && x < 16 && y >= 0 && y < 16) fr[y * 16 + x] = "#7cc4ff"; } } return fr; });
    const typing = [0, 1, 2, 3, 4, 5].map((f) => {
      const fr = blankFrame(16);
      for (let y = 3; y <= 11; y++) for (let x = 1; x <= 14; x++) fr[y * 16 + x] = y === 3 || y === 11 || x === 1 || x === 14 ? "#1e2433" : "#ffffff";
      [[3, 12], [3, 13], [4, 12]].forEach(([x, y]) => { fr[y * 16 + x] = "#1e2433"; });
      [4, 7, 10].forEach((x, i) => { const y = i === f % 3 ? 6 : 7; fr[y * 16 + x] = "#56627d"; fr[y * 16 + x + 1] = "#56627d"; fr[(y + 1) * 16 + x] = "#56627d"; fr[(y + 1) * 16 + x + 1] = "#56627d"; });
      return fr;
    });
    return [makeUserGif("Rec blink", 16, rec, 4), makeUserGif("Sound waves", 16, waves, 6), makeUserGif("Typing dots", 16, typing, 5)].map((g) => Object.assign(g, { desc: "From the Streaming pack" }));
  } }
];
const addedPacks = new Set();

// A built-in GIF drawn out as square frames of colours, like a GIF made in the GIF maker
function builtinFrames(g) {
  const size = Math.max(g.w || 16, g.h || 16), top = Math.floor((size - (g.h || 16)) / 2);
  const frames = [];
  for (let f = 0; f < (g.n || 1); f++) {
    const fr = blankFrame(size);
    if (g.bg) for (let y = 0; y < (g.h || 16); y++) for (let x = 0; x < (g.w || 16); x++) fr[(y + top) * size + x] = g.bg;
    (g.gen ? g.gen(f) : []).forEach(([x, y, c]) => { if (x >= 0 && y >= 0 && x < (g.w || 16) && y < (g.h || 16)) fr[(y + top) * size + x] = c; });
    frames.push(fr);
  }
  return { size, frames, fps: Math.max(1, Math.round(1000 / (g.ms || 125))) };
}
// Built-in GIFs are drawn by code, so phones get them as frames (11-persistence.js saves them
// with the library for the buttons and designs that use them). Drawn once per GIF.
const isBuiltinGif = (g) => !!g && g.kind === "gif" && !g.user && !g.file && !g.pluginId && typeof g.gen === "function";
const BUILTIN_FRAMES = new Map();
function builtinGifEntry(g) {
  if (!BUILTIN_FRAMES.has(g.id)) { const b = builtinFrames(g); BUILTIN_FRAMES.set(g.id, { id: g.id, name: g.name, size: b.size, fps: b.fps, frames: b.frames }); }
  return BUILTIN_FRAMES.get(g.id);
}
// A built-in GIF can't be changed, so Edit makes your own copy of it, drawn the same way
function copyBuiltinGif(g) {
  const b = builtinFrames(g);
  return makeUserGif(`${g.name} copy`.slice(0, 24), b.size, b.frames, b.fps);
}
