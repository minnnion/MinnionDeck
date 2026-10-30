// ===== Images and designs =====
// Sample pictures are small pixel scenes. Uploaded pictures and GIF files live only while the page is open.
function svgURI(w, h, rects) {
  const body = rects.map(([x, y, ww, hh, c]) => `<rect x="${x}" y="${y}" width="${ww}" height="${hh}" fill="${c}"/>`).join("");
  return `data:image/svg+xml,${encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${w} ${h}" width="${w * 8}" height="${h * 8}" shape-rendering="crispEdges" preserveAspectRatio="xMidYMid slice">${body}</svg>`)}`;
}
const SAMPLE_IMAGES = [
  { id: "s:sunset", name: "Sunset", src: svgURI(32, 24, (() => {
    const r = [[0, 0, 32, 6, "#5c3d99"], [0, 6, 32, 5, "#c2457a"], [0, 11, 32, 4, "#ff8a5c"], [0, 15, 32, 9, "#ffc93c"]];
    for (let dy = -5; dy <= 3; dy++) { const half = Math.round(Math.sqrt(25 - dy * dy)); r.push([16 - half, 15 + dy, half * 2, 1, "#fff3b0"]); }
    for (let x = 0; x < 32; x++) { const h = 4 + Math.round(2.5 * Math.sin(x / 3.5) + 1.5 * Math.sin(x / 1.7)); r.push([x, 24 - h, 1, h, "#2d1e4a"]); }
    return r;
  })()) },
  { id: "s:city", name: "Night city", src: svgURI(32, 24, (() => {
    const r = [[0, 0, 32, 24, "#141a33"]], R = pixRng(21);
    for (let i = 0; i < 10; i++) r.push([Math.floor(R() * 32), Math.floor(R() * 9), 1, 1, "#ffffff"]);
    let x = 0;
    while (x < 32) { const w = 3 + Math.floor(R() * 4), h = 8 + Math.floor(R() * 12); r.push([x, 24 - h, w, h, "#232b4d"]); for (let wy = 26 - h; wy < 23; wy += 2) for (let wx = x + 1; wx < x + w - 1; wx += 2) if (R() > 0.5) r.push([wx, wy, 1, 1, "#ffc93c"]); x += w + 1; }
    return r;
  })()) },
  { id: "s:hills", name: "Hills", src: svgURI(32, 24, (() => {
    const r = [[0, 0, 32, 24, "#7cc4ff"], [4, 4, 6, 2, "#ffffff"], [6, 3, 3, 1, "#ffffff"], [20, 6, 7, 2, "#ffffff"]];
    for (let x = 0; x < 32; x++) { const h1 = 9 + Math.round(3 * Math.sin(x / 5)); const h2 = 5 + Math.round(2 * Math.sin(x / 3 + 2)); r.push([x, 24 - h1, 1, h1, "#5fd0a8"], [x, 24 - h2, 1, h2, "#3aa585"]); }
    return r;
  })()) },
  { id: "s:space", name: "Space", src: svgURI(32, 24, (() => {
    const r = [[0, 0, 32, 24, "#0e0f1f"]], R = pixRng(9);
    for (let i = 0; i < 18; i++) r.push([Math.floor(R() * 32), Math.floor(R() * 24), 1, 1, i % 4 ? "#ffffff" : "#ffc93c"]);
    for (let dy = -6; dy <= 6; dy++) { const half = Math.round(Math.sqrt(36 - dy * dy)); r.push([20 - half, 12 + dy, half * 2, 1, dy < 0 ? "#b79cff" : "#8a6fe0"]); }
    r.push([10, 11, 20, 2, "#ffb38a"]);
    return r;
  })()) },
  { id: "s:checker", name: "Checker", src: svgURI(32, 24, (() => { const r = []; for (let x = 0; x < 8; x++) for (let y = 0; y < 6; y++) r.push([x * 4, y * 4, 4, 4, (x + y) % 2 ? "#ff8fab" : "#ffb38a"]); return r; })()) },
  { id: "s:circuit", name: "Circuit", src: svgURI(32, 24, (() => {
    const r = [[0, 0, 32, 24, "#0b1a10"]], R = pixRng(33);
    for (let i = 0; i < 9; i++) { const y = 2 + Math.floor(R() * 20), x = Math.floor(R() * 20), w = 6 + Math.floor(R() * 10); r.push([x, y, w, 1, "#1f6f4a"], [x + w, y - 3, 1, 4, "#1f6f4a"], [x, y, 1, 1, "#5fd0a8"], [x + w, y - 3, 1, 1, "#5fd0a8"]); }
    return r;
  })()) }
];
const USER_IMAGES = [];
const imgById = (id) => [...SAMPLE_IMAGES, ...USER_IMAGES].find((x) => x.id === id) || null;
// Read a picked file into the page's image list, then call back with its id
function uploadImage(after) {
  const inp = document.createElement("input");
  inp.type = "file";
  inp.accept = "image/png, image/jpeg, image/gif, image/webp";
  inp.addEventListener("change", () => {
    const f = inp.files && inp.files[0];
    if (!f) return;
    const rd = new FileReader();
    rd.onload = () => {
      const img = new Image();
      img.onload = () => { const id = `u:${USER_IMAGES.length + 1}`; USER_IMAGES.push({ id, name: f.name.replace(/\.[^.]+$/, ""), src: rd.result, ar: img.naturalWidth / img.naturalHeight || 1 }); after(id); };
      img.src = rd.result;
    };
    rd.readAsDataURL(f);
  });
  inp.click();
}
const esc = (t) => String(t).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);// Designs made in the Button creator. Positions and sizes are percentages of the button.
// A design has its own face for each button size (1 × 1, 2 × 1, 1 × 2, 2 × 2): a background and
// layers. d.bg and d.layers are the face for d.w × d.h, the size open in the designer; d.faces
// holds the other sizes, keyed "2x1". Editing one size never changes another.
const DESIGNS = [];
const designById = (id) => DESIGNS.find((d) => d.id === id) || null;
const DESIGN_SIZES = [[1, 1], [2, 1], [1, 2], [2, 2]];
const sizeKey = (w, h) => `${w}x${h}`;
// Every size gets a face: sizes without one start as a copy of the open face
function ensureFaces(d) {
  d.faces = d.faces || {};
  const main = sizeKey(d.w, d.h);
  delete d.faces[main];
  DESIGN_SIZES.forEach(([w, h]) => { const k = sizeKey(w, h); if (k !== main && !d.faces[k]) d.faces[k] = clone({ bg: d.bg, layers: d.layers }); });
  return d;
}
// The face a button of this size shows (anything else has d.bg and d.layers)
function faceFor(d, w, h) {
  if (!w || !h || sizeKey(w, h) === sizeKey(d.w, d.h)) return d;
  return (d.faces && d.faces[sizeKey(w, h)]) || d;
}
const allFaces = (d) => [d, ...Object.values(d.faces || {})];
// Open another size in the designer: the open face is put away and that size's face comes out
function switchFace(d, w, h) {
  ensureFaces(d);
  const k = sizeKey(w, h);
  if (k === sizeKey(d.w, d.h)) return;
  d.faces[sizeKey(d.w, d.h)] = { bg: d.bg, layers: d.layers };
  const f = d.faces[k];
  delete d.faces[k];
  Object.assign(d, { bg: f.bg, layers: f.layers, w, h });
}
const mixHex = (a, b, t) => "#" + [1, 3, 5].map((i) => Math.round(parseInt(a.slice(i, i + 2), 16) * (1 - t) + parseInt(b.slice(i, i + 2), 16) * t).toString(16).padStart(2, "0")).join("");
function bandsCss(c1, c2) {
  const a = asHex(c1), b = asHex(c2);
  return `linear-gradient(to bottom, ${a} 0 25%, ${mixHex(a, b, 1 / 3)} 25% 50%, ${mixHex(a, b, 2 / 3)} 50% 75%, ${b} 75% 100%)`;
}
function designBg(bg) {
  const im = bg.type === "image" && imgById(bg.image);
  if (im) return `background:${bgCss(bg.colour)} url('${im.src}') center / ${bg.fit === "contain" ? "contain" : "cover"} no-repeat;image-rendering:${im.id.startsWith("s:") ? "pixelated" : "auto"};`;
  if (bg.type === "bands") return `background:${bandsCss(bg.colour, bg.colour2)};`;
  return `background:${bgCss(bg.colour)};`;
}
// A text layer's "{live}" is the button's live title (what its plugin shows now, Host::set_title):
// a knob's speed or altitude, say. In the designer it stays "{live}".
function layerHTML(l, extraCls = "", live) {
  // Text is as wide as what it says, centred on its point (so its outline in the designer fits
  // it, and the phone can draw it the same way); other layers are l.w percent of the width
  const width = l.type === "text" ? "max-content" : `${cssNum(l.w, 60)}%`;
  // A border (stroke): round a box layer's edge, round the letters of a text
  const sk = strokeOf(l.stroke);
  const skCss = !sk ? "" : l.type === "text" ? `-webkit-text-stroke:${sk.w}cqh ${sk.c};paint-order:stroke fill;` : `--sw:${sk.w}cqh;--sc:${sk.c};`;
  const pos = `left:${cssNum(l.x, 50)}%;top:${cssNum(l.y, 50)}%;width:${width};transform:translate(-50%, -50%) rotate(${cssNum(l.rot, 0)}deg);${skCss}`;
  const base = `class="dl dl-${l.type}${sk && l.type !== "text" ? " has-stroke" : ""} ${extraCls}" data-l="${esc(l.id)}"`;
  if (l.type === "text") return `<span ${base.replace('class="dl', `class="dl fx-${cssIdent(l.fx, "none")}${l.font === "mono" ? " mono" : ""}`)} style="${pos}font-size:${cssNum(l.size, 30)}cqh;color:${cssColour(l.colour)};font-weight:${l.bold ? 700 : 500}">${wrapAnim(l, esc(live === undefined ? l.text : String(l.text).replace("{live}", live)))}</span>`;
  // An icon keeps its picture's shape (ar: width / height; square when not set)
  if (l.type === "icon") return `<span ${base} style="${pos}aspect-ratio:${cssNum(l.ar, 1)};color:${cssColour(l.colour)}">${wrapAnim(l, glyph(l.glyph, 8))}</span>`;
  if (l.type === "gif" && GIF[l.gif]) { const g = GIF[l.gif]; return `<span ${base} style="${pos}aspect-ratio:${cssNum(g.w, 1)} / ${cssNum(g.h, 1)}">${wrapAnim(l, gifStrip(g, l.speed))}</span>`; }
  if (l.type === "image") { const im = imgById(l.image); if (!im) return ""; return `<span ${base} style="${pos}aspect-ratio:${cssNum(im.ar, 4 / 3)}">${wrapAnim(l, `<img src="${im.src}" alt="">`)}</span>`; }
  return "";
}
// A design as HTML; with a button size, that size's face; with a live title, "{live}" texts show it
function designHTML(d, w, h, live) {
  const f = faceFor(d, w, h);
  const under = f.bg.type === "gif" && GIF[f.bg.gif] ? `<span class="gif-bg">${gifStrip(GIF[f.bg.gif], f.bg.speed)}</span>` : "";
  // The button's own border sits under the layers, in the editor's canvas as on the board
  const sk = strokeOf(f.bg.stroke);
  const edge = sk ? `<span class="dsg-stroke" style="--sw:${sk.w}cqh;--sc:${sk.c}"></span>` : "";
  return `<span class="dsg" aria-hidden="true" style="${designBg(f.bg)}">${under}${edge}${f.layers.filter((l) => !l.hidden).map((l) => layerHTML(l, "", live)).join("")}</span>`;
}
// A border as it is kept: { c: "#rrggbb", w: thickness in % of the button's height }. Anything
// else (a design from an old or edited file) is no border, so nothing odd reaches a style.
function strokeOf(s) {
  if (!s || typeof s.c !== "string" || !/^#[0-9a-f]{3,8}$/i.test(s.c)) return null;
  const w = Number(s.w);
  return Number.isFinite(w) && w > 0 ? { c: s.c, w: Math.min(w, 50) } : null;
}
// What fills a button state: a colour, a picture, a GIF, or a design
function stateBg(st) {
  const k = st.bgType || "colour";
  const out = { css: bgCss(st.bg), under: "", design: null, pic: false };
  if (k === "image" && st.bgImage && imgById(st.bgImage.src)) {
    const im = imgById(st.bgImage.src);
    out.css = `${bgCss(st.bg)} url('${im.src}') center / ${st.bgImage.fit === "contain" ? "contain" : "cover"} no-repeat`;
    out.pic = true;
  }
  if (k === "gif" && st.bgGif && GIF[st.bgGif.id]) { out.under = `<span class="gif-bg" aria-hidden="true">${gifStrip(GIF[st.bgGif.id], st.bgGif.speed)}</span>`; out.pic = true; }
  if (k === "design" && designById(st.design)) out.design = designById(st.design);
  return out;
}
let layerSeq = 0;
const L = (type, props) => ({ id: `l${++layerSeq}`, type, x: 50, y: 50, w: 60, rot: 0, hidden: false, la: { type: "none", speed: "normal", delay: 0, loop: false }, ...props });
const TX = (text, props) => L("text", { text, size: 30, colour: "#ffffff", bold: true, font: "pixel", fx: "none", ...props });
function newDesign(name, w, h, bg, layers) { return ensureFaces({ id: `d${Date.now().toString(36)}${DESIGNS.length}`, name, w, h, bg: { colour: "#2d3142", colour2: "#5c6bc0", fit: "cover", speed: "normal", ...bg }, layers, updated: Date.now() }); }
// Example designs, as if made in the Button creator
DESIGNS.push(
  Object.assign(newDesign("Sunset BRB", 1, 1, { type: "image", image: "s:sunset" }, [L("gif", { gif: "minnion", x: 50, y: 36, w: 42 }), TX("BRB", { x: 50, y: 78, size: 26, fx: "outline" })]), { id: "d-sunset" }),
  Object.assign(newDesign("On air", 2, 1, { type: "colour", colour: "#2d3142" }, [L("icon", { glyph: "broadcast", colour: "#ff5a5f", x: 18, y: 50, w: 22 }), TX("ON AIR", { x: 60, y: 50, size: 38, fx: "shadow" }), TX("●", { x: 92, y: 18, size: 18, colour: "#ff5a5f", fx: "blink" })]), { id: "d-onair" }),
  Object.assign(newDesign("Neon mute", 1, 1, { type: "bands", colour: "#5c6bc0", colour2: "#ff8fab" }, [L("icon", { glyph: "mic", colour: "#ffffff", x: 50, y: 40, w: 40 }), TX("MUTE", { x: 50, y: 80, size: 20 })]), { id: "d-neon" }),
  Object.assign(newDesign("Lo-fi", 1, 1, { type: "gif", gif: "stars", speed: "slow" }, [L("icon", { glyph: "note", colour: "#ffc93c", x: 50, y: 42, w: 36 }), TX("LO-FI", { x: 50, y: 80, size: 18, font: "pixel" })]), { id: "d-lofi" }),
  Object.assign(newDesign("Clip it", 1, 1, { type: "gif", gif: "fire" }, [TX("CLIP", { x: 50, y: 40, size: 30, fx: "outline" }), TX("30s", { x: 50, y: 68, size: 16, font: "mono", fx: "outline" })]), { id: "d-clip" })
);
// A logo animation: the M logo pops in and shines, the name types itself, then DECK slides up. It plays back and forth.
DESIGNS.unshift(Object.assign(newDesign("Minniondeck logo", 2, 1, { type: "bands", colour: "#1e2433", colour2: "#5c6bc0" }, [
  L("gif", { gif: USER_GIFS[2].id, x: 20, y: 50, w: 30, la: { type: "pop", speed: "normal", delay: 0, loop: true } }),
  TX("MINNION", { x: 63, y: 40, size: 30, fx: "shadow", la: { type: "type", speed: "normal", delay: 300, loop: true } }),
  TX("DECK", { x: 63, y: 72, size: 20, colour: "#5fd0a8", la: { type: "slideup", speed: "normal", delay: 1000, loop: true } })
]), { id: "d-logo" }));
const designUses = (id) => Object.values(boards).flat().filter((t) => t.states.some((st) => st.bgType === "design" && st.design === id));


// ===== Pixel pictures, drawn in the pixel editor (it was in the app; it is on the website now) =====
// A pixel picture is kept as a PNG in your pictures (USER_IMAGES), so it works anywhere a picture
// does: button backgrounds and design layers. Its pixels are kept too, so it can be edited again.
let picSeq = 0;
function picToPng(size, px) {
  const c = document.createElement("canvas");
  c.width = size; c.height = size;
  const ctx = c.getContext("2d");
  px.forEach((col, i) => { if (col) { ctx.fillStyle = col; ctx.fillRect(i % size, Math.floor(i / size), 1, 1); } });
  return c.toDataURL("image/png");
}
function makePicture(name, size, px) {
  const m = { id: `u:p${Date.now().toString(36)}${++picSeq}`, name, src: picToPng(size, px), ar: 1, pixel: { size, px } };
  USER_IMAGES.push(m);
  return m;
}
// A copy of a picture as a pixel picture (for editing a built-in one). Resolves to the new picture.
function pictureFromSrc(src, name, size = 32) {
  return new Promise((resolve) => {
    const img = new Image();
    img.onload = () => {
      const c = document.createElement("canvas");
      c.width = size; c.height = size;
      const ctx = c.getContext("2d");
      ctx.imageSmoothingEnabled = false;
      // Fitted inside the square, centred, without stretching
      const k = Math.min(size / (img.naturalWidth || size), size / (img.naturalHeight || size));
      const w = Math.round((img.naturalWidth || size) * k), h = Math.round((img.naturalHeight || size) * k);
      ctx.drawImage(img, Math.floor((size - w) / 2), Math.floor((size - h) / 2), w, h);
      const data = ctx.getImageData(0, 0, size, size).data;
      const hex = (n) => n.toString(16).padStart(2, "0");
      const px = [];
      for (let i = 0; i < size * size; i++) px.push(data[i * 4 + 3] < 128 ? null : `#${hex(data[i * 4])}${hex(data[i * 4 + 1])}${hex(data[i * 4 + 2])}`);
      resolve(makePicture(name.slice(0, 24), size, px));
    };
    img.onerror = () => resolve(makePicture(name.slice(0, 24), size, new Array(size * size).fill(null)));
    img.src = src;
  });
}
