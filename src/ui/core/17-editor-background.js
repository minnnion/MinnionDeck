// ----- A state's background colour, and its Image section -----
// The background is a colour. On top of it, from the Image section under Look, goes a button
// design you made, or an icon or a picture. An icon becomes this button's own small design (the
// button's colour, the icon and its name). Adjust changes the design's colours, sizes and where
// each part sits; a design that other buttons share is copied for this button first. GIFs and
// widgets have the GIF section (19-editor-gif.js). New pictures are made on the Minniondeck
// website and come in with Import.
function renderBg(st) {
  const k = st.bgType || "colour";
  $("ed-bg-colour-field").hidden = k === "design";
  $("cap-bg").textContent = k === "image" ? "Base colour, behind see-through parts" : "Colour";
  colourPicker($("ed-bg"), st);
  renderImage();
}

// GIFs and pictures are made on the Minniondeck website ("gifs" or "images" page, opened in the
// browser); the files they give come back with Import
function openSite(page) {
  backend.run("open_site", { page }).then(() => say("Opened the Minniondeck website. Download what you make, then use Import here.")).catch((e) => say(String(e)));
}

// ----- The two dropdowns: Button design, and Icon or picture -----
let dpOpen = false, dpActive = null, dpOpts = [];
let icOpen = false, icActive = null, icOpts = [], icSearch = "", icAll = false;
let adjOpen = false, adjPart = "bg";
const PX_SHOWN = 240;
const ADJ_COLOURS = ["#ffffff", "#1e2433", "#2d3142", "#5fd0a8", "#7cc4ff", "#b79cff", "#ffb38a", "#ffc93c", "#ff5a5f", "#c93c41", "#9ad94b", "#3ec1d3", "#5c6bc0", "#ff8fab", "#f4a259", "#8d6e63"];

// A design made for one button (from an icon, or a copy made by Adjust) is marked "own". It stays
// out of the design lists and is only saved while a button uses it (11-persistence.js).
const ownDesign = (st) => { const d = st.bgType === "design" && designById(st.design); return d && d.own ? d : null; };
const iconLabel = (id) => (id.startsWith("px:") ? pxName(id) : id.startsWith("si:") ? (siOf(id) || [, (artOf(id) || {}).name || id.slice(3)])[1] : ICON_NAMES[id] || id.split(":").pop().replace(/\.[^.]+$/, ""));
function designKey(st) {
  if (st.bgType !== "design" || !designById(st.design)) return "";
  const own = ownDesign(st);
  return own && own.fromIcon ? "" : `d:${st.design}`;
}
function iconKey(st) {
  const own = ownDesign(st);
  if (own && own.fromIcon) return `i:${own.fromIcon}`;
  if (st.bgType === "image" && st.bgImage && imgById(st.bgImage.src)) return `p:${st.bgImage.src}`;
  return "";
}
function designGroups(st) {
  const own = ownDesign(st);
  return [
    ["Off", [{ key: "", name: "None", desc: "No design" }]],
    ...(own && !own.fromIcon ? [["This button", [{ key: `d:${own.id}`, name: own.name, desc: "This button's own copy", design: own }]]] : []),
    ["Your designs", DESIGNS.filter((d) => !d.own).map((d) => ({ key: `d:${d.id}`, name: d.name, desc: `Button design, ${d.w} × ${d.h}`, design: d }))]
  ].filter(([, items]) => items.length);
}
function designItem(st) {
  const k = designKey(st);
  return designGroups(st).flatMap(([, items]) => items).find((x) => x.key === k) || null;
}
// Icon groups: your pictures, the built-in pictures, the app's icons (and plugin icons), the pixel icon set
function iconGroups() {
  const pics = (list, desc, builtIn) => list.map((m) => ({ key: `p:${m.id}`, name: m.name, desc: m.pixel ? "Pixel picture" : desc, pic: m, builtIn }));
  return [
    ["Your pictures", pics(USER_IMAGES, "Added picture", false)],
    ["Built-in pictures", pics(SAMPLE_IMAGES, "Built-in picture", true)],
    ...ICON_SETS.map((set) => [set.plugin ? set.name : "App icons", set.icons.filter((id) => !set.plugin || pluginState(set.plugin) !== "missing").map((id) => ({ key: `i:${id}`, name: iconLabel(id), desc: set.plugin ? `${set.name} icon` : "App icon", glyph: id }))]),
    // Logos drawn only by hand (not in Simple Icons, like Microsoft Flight Simulator 2024) come first
    ["App and game logos", Object.keys(BRAND_ART).filter((slug) => !siIcons().has(slug)).map((slug) => ({ key: `i:si:${slug}`, name: BRAND_ART[slug].name || slug, desc: "App or game logo", glyph: `si:${slug}` })).concat(SI_ICONS.map(([slug, name]) => ({ key: `i:si:${slug}`, name, desc: "App or game logo", glyph: `si:${slug}` })))],
    ["Pixel icons", [...pxIcons().keys()].map((n) => ({ key: `i:px:${n}`, name: pxName(`px:${n}`), desc: "Pixel icon", glyph: `px:${n}` }))]
  ].filter(([, items]) => items.length);
}
function iconItem(st) {
  const k = iconKey(st);
  if (!k) return null;
  if (k.startsWith("i:")) return { key: k, name: iconLabel(k.slice(2)), desc: "Icon, as this button's design", glyph: k.slice(2) };
  const m = imgById(k.slice(2));
  return m ? { key: k, name: m.name, desc: m.pixel ? "Pixel picture" : m.id.startsWith("s:") ? "Built-in picture" : "Added picture", pic: m, builtIn: m.id.startsWith("s:") } : null;
}
function itemMini(item) {
  if (!item || (!item.design && !item.glyph && !item.pic)) return `<span class="gmini" aria-hidden="true">${glyph("cross", 14)}</span>`;
  if (item.design) return `<span class="gmini imini" aria-hidden="true">${designHTML(item.design)}</span>`;
  if (item.glyph) return `<span class="gmini" aria-hidden="true">${glyph(item.glyph, 22)}</span>`;
  return `<span class="gmini imini" aria-hidden="true" style="background-image:url('${item.pic.src}')"></span>`;
}

// What goes on the state for a key: "" (just the colour), "d:<design>", "p:<picture>" or "i:<icon>"
function setImage(st, key) {
  if (!key) { st.bgType = "colour"; return; }
  if (key.startsWith("d:")) { st.bgType = "design"; st.design = key.slice(2); return; }
  if (key.startsWith("i:")) {
    const gl = key.slice(2), own = ownDesign(st);
    // Already an icon design: swap the icon and keep what Adjust changed
    if (own && own.fromIcon) {
      // On every size's face. A logo brings its brand colour, with its icon and text in the
      // colour that reads best on it.
      const brand = gl.startsWith("si:") && siBg(gl);
      allFaces(own).forEach((f) => {
        const l = f.layers.find((x) => x.type === "icon");
        if (l) l.glyph = gl;
        if (brand) {
          f.bg = { ...f.bg, type: "colour", colour: brand };
          f.layers.forEach((x) => { if (x.type === "icon" || x.type === "text") x.colour = textOn(f.bg.colour); });
        }
      });
      own.fromIcon = gl;
      own.name = `${iconLabel(gl)} icon`;
      own.updated = Date.now();
      return;
    }
    const d = iconDesign(st, gl);
    st.bgType = "design";
    st.design = d.id;
    return;
  }
  st.bgImage = { src: key.slice(2), fit: (st.bgImage && st.bgImage.fit) || "cover" };
  st.bgType = "image";
}
// An icon as a small design: the state's colour, the icon in the colour that reads best on it,
// and the button's name with it when the button shows names. Each size gets a layout that fits
// it: the name beside the icon on a wide button, under it on the others.
function iconDesign(st, gl) {
  // A logo sits on its brand colour; any other icon on the button's colour
  const brand = gl.startsWith("si:") && siBg(gl);
  const bg = brand || asHex(st.bg);
  const ink = textOn(bg);
  const name = draft.style.shape !== "icon" ? String(st.text || draft.label || "").trim().slice(0, 14) : "";
  const layout = (w, h) => {
    const wide = w > h, tall = h > w;
    if (!name) return [L("icon", { glyph: gl, colour: ink, x: 50, y: 50, w: wide ? 30 : 62 })];
    if (wide) return [L("icon", { glyph: gl, colour: ink, x: 25, y: 50, w: 22 }), TX(name, { x: 64, y: 50, w: 66, size: 22, colour: ink })];
    if (tall) return [L("icon", { glyph: gl, colour: ink, x: 50, y: 38, w: 58 }), TX(name, { x: 50, y: 74, size: 10, colour: ink })];
    return [L("icon", { glyph: gl, colour: ink, x: 50, y: 40, w: 46 }), TX(name, { x: 50, y: 82, size: 18, colour: ink })];
  };
  const w = draft.w || 1, h = draft.h || 1;
  const d = newDesign(`${iconLabel(gl)} icon`, w, h, { type: "colour", colour: bg }, layout(w, h));
  Object.values(d.faces).forEach((f) => { f.layers = layout(...DESIGN_SIZES.find(([a, b]) => d.faces[sizeKey(a, b)] === f)); });
  Object.assign(d, { own: true, fromIcon: gl });
  DESIGNS.push(d);
  return d;
}

// A plugin choice's picture as this button's own design (as an icon design, so the Image section
// can change it and the creator edit it): the picture as large as fits, keeping its shape (ar,
// width / height), with the state's text under it when there is some ("{live}" shows the
// plugin's live title). Each button size gets a layout that fits it.
function pictureDesign(gl, ar, text, name) {
  ar = ar > 0 ? ar : 1;
  const layout = (w, h) => {
    const room = text ? 70 : 92; // percent of the height the picture may take
    // Layer sizes are percent of the face's width; the face is w x h button cells
    const iw = Math.min(94, (room * ar * h) / w);
    const ih = (iw * w) / (ar * h);
    const y = text ? 3 + ih / 2 : 50;
    const layers = [L("icon", { glyph: gl, ar, colour: "#ffffff", x: 50, y, w: Math.round(iw * 10) / 10 })];
    if (text) layers.push(TX(text, { x: 50, y: Math.min(90, 3 + ih + (97 - 3 - ih) / 2), size: 17, colour: "#ffffff", font: "pixel" }));
    return layers;
  };
  const w = draft.w || 1, h = draft.h || 1;
  const d = newDesign(name, w, h, { type: "colour", colour: "#2d3142" }, layout(w, h));
  Object.values(d.faces).forEach((f) => { f.layers = layout(...DESIGN_SIZES.find(([a, b]) => d.faces[sizeKey(a, b)] === f)); });
  Object.assign(d, { own: true, fromIcon: gl });
  DESIGNS.push(d);
  return d;
}

// A dropdown button like the others in the Edit window
function pickButton(id, capId, item, emptyName, emptyDesc) {
  const wrap = Object.assign(document.createElement("div"), { className: "pick", id: `${id}` });
  const btn = Object.assign(document.createElement("button"), { type: "button", className: "pick-btn", id: `${id}-btn` });
  btn.setAttribute("aria-haspopup", "listbox");
  btn.setAttribute("aria-expanded", "false");
  btn.setAttribute("aria-labelledby", `${capId} ${id}-btn`);
  btn.innerHTML = `${itemMini(item)}<span class="pick-txt"><span></span><small></small></span>${glyph("caret", 16)}`;
  btn.querySelector(".pick-txt span").textContent = item ? item.name : emptyName;
  btn.querySelector(".pick-txt small").textContent = item ? item.desc : emptyDesc;
  const pop = Object.assign(document.createElement("div"), { className: "pick-pop", id: `${id}-pop` });
  pop.hidden = true;
  wrap.append(btn, pop);
  return { wrap, btn, pop };
}

function renderImage() {
  if (!draft) return;
  const st = draft.states[stSel];
  const multi = draft.states.length > 1;
  $("ed-img-state-field").hidden = !multi;
  if (multi) segment($("ed-img-state"), draft.states.map((s, i) => [i, s.name]), stSel, (i) => { selectState(i); const b = $("ed-img-state").querySelector('[aria-pressed="true"]'); if (b) b.focus(); });
  const dItem = designItem(st), iItem = iconItem(st);

  // Button design
  const dArea = $("ed-dsg-area");
  dArea.innerHTML = "";
  dpOpen = false;
  const dp = pickButton("ed-dpick", "cap-dsg", dItem, "None", "No design");
  const dList = Object.assign(document.createElement("div"), { className: "picker", id: "ed-dsgs", tabIndex: -1 });
  dList.setAttribute("role", "listbox");
  dList.setAttribute("aria-labelledby", "cap-dsg");
  dp.pop.appendChild(dList);
  dArea.appendChild(dp.wrap);
  dp.btn.addEventListener("click", () => (dpOpen ? closeDPick(true) : openDPick()));
  dp.btn.addEventListener("keydown", (e) => { if (e.key === "ArrowDown" || e.key === "ArrowUp") { e.preventDefault(); openDPick(); } });
  dList.addEventListener("keydown", (e) => {
    const i = dpOpts.findIndex((o) => o.dataset.key === dpActive);
    if (e.key === "ArrowDown") setDActive(i + 1);
    else if (e.key === "ArrowUp") setDActive(i - 1);
    else if (e.key === "Home") setDActive(0);
    else if (e.key === "End") setDActive(dpOpts.length - 1);
    else if (e.key === "Enter" || e.key === " ") chooseImage(dpActive, "ed-dpick-btn");
    else if (e.key === "Escape") { e.stopPropagation(); closeDPick(true); }
    else if (e.key === "Tab") { closeDPick(false); return; }
    else return;
    e.preventDefault();
  });

  // Icon or picture, with a search box
  const iArea = $("ed-img-area");
  iArea.innerHTML = "";
  icOpen = false;
  const ip = pickButton("ed-impick", "cap-img", iItem, "None", "No icon or picture");
  const search = Object.assign(document.createElement("input"), { id: "ed-imsearch", type: "search", placeholder: `Search ${iconGroups().reduce((n, [, items]) => n + items.length, 0)} icons and pictures`, autocomplete: "off" });
  search.setAttribute("role", "combobox");
  search.setAttribute("aria-label", "Search icons and pictures");
  search.setAttribute("aria-autocomplete", "list");
  search.setAttribute("aria-controls", "ed-imgs");
  search.setAttribute("aria-expanded", "false");
  const iList = Object.assign(document.createElement("div"), { className: "picker", id: "ed-imgs" });
  iList.setAttribute("role", "listbox");
  iList.setAttribute("aria-labelledby", "cap-img");
  ip.pop.append(search, iList);
  iArea.appendChild(ip.wrap);
  ip.btn.addEventListener("click", () => (icOpen ? closeImPick(true) : openImPick()));
  ip.btn.addEventListener("keydown", (e) => { if (e.key === "ArrowDown" || e.key === "ArrowUp") { e.preventDefault(); openImPick(); } });
  search.addEventListener("input", () => { icSearch = search.value; icActive = null; buildIconCells(); });
  search.addEventListener("keydown", (e) => {
    const i = icOpts.findIndex((o) => o.dataset.key === icActive);
    if (e.key === "ArrowRight") setIcActive(i + 1);
    else if (e.key === "ArrowLeft") setIcActive(i - 1);
    else if (e.key === "ArrowDown") setIcActive(icNeighbour(i, 1));
    else if (e.key === "ArrowUp") setIcActive(icNeighbour(i, -1));
    else if (e.key === "Enter") { if (icActive !== null) chooseIconCell(icActive); }
    else if (e.key === "Escape") { e.stopPropagation(); closeImPick(true); }
    else if (e.key === "Tab") { closeImPick(false); return; }
    else return;
    e.preventDefault();
  });

  // Fit, only for pictures
  const isPic = st.bgType === "image" && iItem && iItem.pic;
  $("ed-img-fit-field").hidden = !isPic;
  if (isPic) segment($("ed-img-fit"), [["cover", "Fill the button"], ["contain", "Show it all"]], st.bgImage.fit || "cover", (v) => { st.bgImage.fit = v; renderImage(); renderStates(); refreshAll(); const b = $("ed-img-fit").querySelector('[aria-pressed="true"]'); if (b) b.focus(); });

  // Adjust, Import, and Make on the website
  const tools = $("ed-img-tools");
  tools.innerHTML = "";
  const tb = (label, fn, aria) => { const b = Object.assign(document.createElement("button"), { type: "button", className: "btn sm", textContent: label }); if (aria) b.setAttribute("aria-label", aria); b.addEventListener("click", fn); return b; };
  const onDesign = st.bgType === "design" && designById(st.design);
  const adj = tb("Adjust", () => { adjOpen = !adjOpen; renderImage(); const b = $("ed-img-adj"); if (b) b.focus(); }, "Adjust colours, sizes and places");
  adj.id = "ed-img-adj";
  adj.setAttribute("aria-expanded", String(adjOpen && !!onDesign));
  adj.setAttribute("aria-controls", "ed-adjust");
  adj.disabled = !onDesign;
  const imp = tb("Import…", () => uploadImage((id) => { setImage(draft.states[stSel], `p:${id}`); afterImageChange(); say("Picture added and put on the button"); }), "Import a picture file");
  const web = tb("Make on the website", () => openSite("images"), "Make a picture on the Minniondeck website");
  tools.append(adj, imp, web);

  renderAdjust(onDesign && adjOpen ? designById(st.design) : null);
  const own = ownDesign(st);
  $("ed-img-help").textContent = !onDesign && !iItem ? "Pick a design, or an icon or picture, to go on top of the colour. Make new pictures on the website, then Import them."
    : own && own.fromIcon ? "The icon is this button's own design. Adjust changes its colours, sizes and places."
    : own ? "This button has its own copy of the design. Adjust changes it here."
    : onDesign ? "A design is the whole face of the button. Adjust makes a copy for this button first, so other buttons stay the same."
    : "To change this picture, make a new one on the website and bring it in with Import.";
  $("sum-image").textContent = dItem && dItem.key ? dItem.name : iItem ? iItem.name : "None";
}
// After the state's image changes: the section, the state list, the preview
function afterImageChange() {
  renderBg(draft.states[stSel]);
  renderStates();
  refreshAll();
  flipPreview();
}
function closeOtherPicks() { closePick(false); closeIPick(false); closeAPick(false); closeGPick(false); closeDPick(false); closeImPick(false); }

// ----- Design dropdown: a list -----
function openDPick() {
  if (dpOpen || !$("ed-dpick-btn")) return;
  closeOtherPicks();
  dpOpen = true;
  const list = $("ed-dsgs");
  const cur = designKey(draft.states[stSel]);
  list.innerHTML = "";
  dpOpts = [];
  designGroups(draft.states[stSel]).forEach(([gname, items], gi) => {
    const group = document.createElement("div");
    group.setAttribute("role", "group");
    group.setAttribute("aria-labelledby", `dsgrp-${gi}`);
    group.appendChild(Object.assign(document.createElement("div"), { className: "grp", id: `dsgrp-${gi}`, textContent: gname }));
    items.forEach((x, k) => {
      const o = Object.assign(document.createElement("div"), { className: "opt", id: `ds-${gi}-${k}` });
      o.dataset.key = x.key;
      o.setAttribute("role", "option");
      o.setAttribute("aria-selected", String(x.key === cur));
      o.innerHTML = `${itemMini(x.design ? x : null)}<span class="opt-txt"><span></span><small></small></span>${x.key === cur ? '<span class="opt-check" aria-hidden="true">✓</span>' : ""}`;
      o.querySelector(".opt-txt span").textContent = x.name;
      o.querySelector(".opt-txt small").textContent = x.desc;
      o.addEventListener("click", () => chooseImage(x.key, "ed-dpick-btn"));
      o.addEventListener("mousemove", () => { if (dpActive !== x.key) setDActive(dpOpts.indexOf(o), false); });
      group.appendChild(o);
      dpOpts.push(o);
    });
    list.appendChild(group);
  });
  showPop($("ed-dpick-pop"));
  $("ed-dpick-btn").setAttribute("aria-expanded", "true");
  setDActive(Math.max(0, dpOpts.findIndex((o) => o.dataset.key === cur)));
  $("ed-dpick-pop").scrollIntoView({ block: "nearest" });
  list.focus({ preventScroll: true });
}
function setDActive(i, scroll = true) {
  if (!dpOpts.length) return;
  const k = Math.max(0, Math.min(dpOpts.length - 1, i));
  dpActive = dpOpts[k].dataset.key;
  dpOpts.forEach((o, j) => o.classList.toggle("active", j === k));
  $("ed-dsgs").setAttribute("aria-activedescendant", dpOpts[k].id);
  if (!scroll) return;
  const list = $("ed-dsgs"), o = dpOpts[k];
  const lr = list.getBoundingClientRect(), orr = o.getBoundingClientRect(), head = 30;
  if (orr.top < lr.top + head) list.scrollTop -= lr.top + head - orr.top;
  else if (orr.bottom > lr.bottom) list.scrollTop += orr.bottom - lr.bottom;
}
function closeDPick(refocus) {
  if (!dpOpen) return;
  dpOpen = false;
  hidePop($("ed-dpick-pop"));
  $("ed-dpick-btn").setAttribute("aria-expanded", "false");
  if (refocus) $("ed-dpick-btn").focus();
}

// ----- Icon dropdown: a grid of cells, searchable -----
function buildIconCells() {
  const list = $("ed-imgs");
  const q = icSearch.trim().toLowerCase();
  const cur = iconKey(draft.states[stSel]);
  list.innerHTML = "";
  icOpts = [];
  let k = 0;
  const groups = [["Off", [{ key: "", name: "None", desc: "Just the colour" }]], ...iconGroups()];
  groups.forEach(([gname, items], gi) => {
    let shown = items.filter((x) => !q || x.name.toLowerCase().includes(q) || x.key.toLowerCase().includes(q) || gname.toLowerCase().includes(q));
    if (q && gname === "Off") shown = [];
    if (!shown.length) return;
    const total = shown.length;
    // The big pixel set shows its first icons until you search or ask for all of them
    const cut = (gname === "Pixel icons" || gname === "App and game logos") && !q && !icAll && total > PX_SHOWN;
    if (cut) shown = shown.slice(0, PX_SHOWN);
    const group = document.createElement("div");
    group.setAttribute("role", "group");
    group.setAttribute("aria-labelledby", `icgrp-${gi}`);
    const head = Object.assign(document.createElement("div"), { className: "grp", id: `icgrp-${gi}` });
    head.innerHTML = `<span></span><small></small>`;
    head.querySelector("span").textContent = gname;
    head.querySelector("small").textContent = gname === "Off" ? "" : `${total} ${total === 1 ? "item" : "items"}`;
    const cells = Object.assign(document.createElement("div"), { className: "icon-cells" });
    shown.forEach((x) => {
      const o = Object.assign(document.createElement("div"), { className: "icell", id: `imc-${k}` });
      o.dataset.key = x.key;
      o.setAttribute("role", "option");
      o.setAttribute("aria-selected", String(x.key === cur));
      o.title = x.desc;
      o.style.setProperty("--d", `${Math.min(k++ * 6, 240)}ms`);
      o.innerHTML = x.pic ? `<span class="act-ico im-thumb" aria-hidden="true" style="background-image:url('${x.pic.src}')"></span><span></span>`
        : x.glyph && x.glyph.startsWith("si:") ? `<span class="act-ico" aria-hidden="true" style="background:${siBg(x.glyph)};color:${textOn(siBg(x.glyph))}">${glyph(x.glyph, 16)}</span><span></span>`
        : x.glyph ? `<span class="act-ico" aria-hidden="true">${glyph(x.glyph, 16)}</span><span></span>`
        : `<span class="act-ico" aria-hidden="true">${glyph("cross", 16)}</span><span></span>`;
      o.querySelector("span:last-child").textContent = x.name;
      o.addEventListener("pointerdown", (e) => e.preventDefault());
      o.addEventListener("click", () => chooseIconCell(x.key));
      o.addEventListener("mousemove", () => { if (icActive !== x.key) setIcActive(icOpts.indexOf(o), false); });
      cells.appendChild(o);
      icOpts.push(o);
    });
    group.append(head, cells);
    if (cut) {
      const more = Object.assign(document.createElement("button"), { type: "button", className: "btn sm ic-more", textContent: `Show all ${total} ${gname === "Pixel icons" ? "pixel icons" : "logos"}` });
      more.addEventListener("pointerdown", (e) => e.preventDefault());
      more.addEventListener("click", () => { icAll = true; buildIconCells(); $("ed-imsearch").focus({ preventScroll: true }); });
      group.appendChild(more);
    }
    list.appendChild(group);
  });
  if (!icOpts.length) {
    list.appendChild(Object.assign(document.createElement("div"), { className: "none", textContent: `Nothing matches "${icSearch}". Try a shorter word.` }));
    icActive = null;
    $("ed-imsearch").removeAttribute("aria-activedescendant");
    return;
  }
  const keep = icOpts.findIndex((o) => o.dataset.key === icActive);
  const at = icOpts.findIndex((o) => o.dataset.key === cur);
  setIcActive(keep >= 0 ? keep : q ? 0 : Math.max(0, at));
}
function icNeighbour(i, dir) {
  if (i < 0) return 0;
  const r = icOpts[i].getBoundingClientRect();
  const cands = icOpts.map((o, j) => [j, o.getBoundingClientRect()]).filter(([, b]) => (dir > 0 ? b.top > r.top + 4 : b.top < r.top - 4));
  if (!cands.length) return i;
  const rowTop = dir > 0 ? Math.min(...cands.map(([, b]) => b.top)) : Math.max(...cands.map(([, b]) => b.top));
  const row = cands.filter(([, b]) => Math.abs(b.top - rowTop) < 4);
  row.sort((a, b) => Math.abs(a[1].left - r.left) - Math.abs(b[1].left - r.left));
  return row[0][0];
}
function setIcActive(i, scroll = true) {
  if (!icOpts.length) return;
  const k = Math.max(0, Math.min(icOpts.length - 1, i));
  icActive = icOpts[k].dataset.key;
  icOpts.forEach((o, j) => o.classList.toggle("active", j === k));
  $("ed-imsearch").setAttribute("aria-activedescendant", icOpts[k].id);
  if (!scroll) return;
  const list = $("ed-imgs"), o = icOpts[k];
  const lr = list.getBoundingClientRect(), orr = o.getBoundingClientRect(), head = 34;
  if (orr.top < lr.top + head) list.scrollTop -= lr.top + head - orr.top;
  else if (orr.bottom > lr.bottom) list.scrollTop += orr.bottom - lr.bottom;
}
function openImPick() {
  if (icOpen || !$("ed-impick-btn")) return;
  closeOtherPicks();
  icOpen = true;
  icSearch = "";
  icActive = null;
  icAll = false;
  const search = $("ed-imsearch"), pop = $("ed-impick-pop");
  search.value = "";
  showPop(pop);
  $("ed-impick-btn").setAttribute("aria-expanded", "true");
  search.setAttribute("aria-expanded", "true");
  buildIconCells();
  pop.scrollIntoView({ block: "nearest" });
  search.focus({ preventScroll: true });
}
function closeImPick(refocus) {
  if (!icOpen) return;
  icOpen = false;
  hidePop($("ed-impick-pop"));
  $("ed-impick-btn").setAttribute("aria-expanded", "false");
  $("ed-imsearch").setAttribute("aria-expanded", "false");
  $("ed-imsearch").removeAttribute("aria-activedescendant");
  if (refocus) $("ed-impick-btn").focus();
}
function chooseIconCell(key) {
  // A design picked before stays where it was; an icon or picture replaces it on this state
  const st = draft.states[stSel];
  if (key && key.startsWith("i:") && st.bgType === "design" && !(ownDesign(st) && ownDesign(st).fromIcon)) st.bgType = "colour";
  chooseImage(key, "ed-impick-btn");
}
function chooseImage(key, back) {
  if (key === null || key === undefined) return;
  setImage(draft.states[stSel], key);
  closeDPick(false);
  closeImPick(false);
  afterImageChange();
  const b = $(back);
  if (b) b.focus();
}
document.addEventListener("pointerdown", (e) => {
  if (dpOpen && !e.target.closest("#ed-dpick")) closeDPick(false);
  if (icOpen && !e.target.closest("#ed-impick")) closeImPick(false);
});

// ----- Adjust: change this button's design without opening the designer -----
// Pick a part (the background, or one of the layers), then its colour, size and place. The first
// change to a design other buttons can use makes this button's own copy of it.
function ownCopy() {
  const st = draft.states[stSel];
  const d = designById(st.design);
  if (!d || d.own) return d;
  const c = JSON.parse(JSON.stringify(ensureFaces(d)));
  // New layer ids, the same on every size's face as before
  const ids = new Map();
  allFaces(c).forEach((f) => f.layers.forEach((l) => { if (!ids.has(l.id)) ids.set(l.id, `l${++layerSeq}`); l.id = ids.get(l.id); }));
  Object.assign(c, { id: `d${Date.now().toString(36)}o${DESIGNS.length}`, name: `${d.name} (${draft.label || "this button"})`, own: true, fromDesign: d.id, updated: Date.now() });
  DESIGNS.push(c);
  st.design = c.id;
  say(`Made a copy of ${d.name} for this button, so other buttons stay the same`);
  return c;
}
const partName = (l, n) => ({ text: "Text", icon: "Icon", gif: "GIF", image: "Picture" })[l.type] + (n > 1 ? ` ${n}` : "");
function renderAdjust(dsg) {
  const box = $("ed-adjust");
  box.hidden = !dsg;
  box.innerHTML = "";
  if (!dsg) return;
  // Only the face for this button's size changes; the design's other sizes stay as they are
  const bw = draft.w || 1, bh = draft.h || 1;
  const d = faceFor(dsg, bw, bh);
  const counts = {};
  const parts = [["bg", "Background"], ...d.layers.map((l, i) => { counts[l.type] = (counts[l.type] || 0) + 1; return [String(i), partName(l, counts[l.type])]; })];
  if (!parts.some(([v]) => v === adjPart)) adjPart = "bg";
  // Changes go to this button's own design; the preview follows
  const change = (fn, rebuild) => {
    const was = designById(draft.states[stSel].design);
    const own = ownCopy();
    // Just copied: the design dropdown and the help say so (rebuilt on the next render)
    if (was !== own && !rebuild) {
      const b = $("ed-dpick-btn");
      if (b) { b.querySelector(".pick-txt span").textContent = own.name; b.querySelector(".pick-txt small").textContent = "This button's own copy"; }
      $("ed-img-help").textContent = "This button has its own copy of the design. Adjust changes it here.";
    }
    fn(faceFor(own, bw, bh));
    own.updated = Date.now();
    renderStates();
    refreshAll();
    if (rebuild) renderImage();
    else { const sum = $("sum-image"); if (sum && !own.fromIcon) sum.textContent = own.name; }
  };
  const field = (label, ...kids) => { const f = Object.assign(document.createElement("div"), { className: "field" }); f.append(Object.assign(document.createElement("span"), { className: "cap", textContent: label }), ...kids); return f; };
  const swatches = (label, value, fn) => {
    const r = Object.assign(document.createElement("div"), { className: "sw-row" });
    r.setAttribute("role", "group");
    r.setAttribute("aria-label", label);
    const hex = asHex(value || "#ffffff");
    ADJ_COLOURS.forEach((c) => {
      const b = Object.assign(document.createElement("button"), { type: "button" });
      b.style.background = c;
      b.setAttribute("aria-label", c.toUpperCase());
      b.setAttribute("aria-pressed", String(hex.toLowerCase() === c));
      b.addEventListener("click", () => { fn(c); r.querySelectorAll("button").forEach((x) => x.setAttribute("aria-pressed", String(x === b))); });
      r.appendChild(b);
    });
    const lab = Object.assign(document.createElement("label"), { title: "Any colour" });
    const inp = Object.assign(document.createElement("input"), { type: "color", value: hex });
    inp.setAttribute("aria-label", `${label}: any colour`);
    inp.addEventListener("input", () => fn(inp.value));
    lab.appendChild(inp);
    r.appendChild(lab);
    return field(label, r);
  };
  const slide = (label, min, max, value, fn) => pxSlider({ label, min, max, value: Math.round(value), unit: "%", onInput: (v) => change((own) => fn(own, v)) });

  const top = Object.assign(document.createElement("div"), { className: "adj-head" });
  box.appendChild(Object.assign(document.createElement("p"), { className: "help", textContent: `Changes the ${bw} × ${bh} face, the size of this button. The design's other sizes stay as they are.` }));
  top.append(Object.assign(document.createElement("span"), { className: "cap", id: "cap-adj", textContent: "Part to change" }));
  const seg = Object.assign(document.createElement("div"), { className: parts.length >= 4 ? "seg seg4 adj-parts" : "seg adj-parts" });
  seg.setAttribute("role", "group");
  seg.setAttribute("aria-labelledby", "cap-adj");
  segment(seg, parts, adjPart, (v) => { adjPart = v; renderAdjust(designById(draft.states[stSel].design)); const pb = $("ed-adjust").querySelector('.adj-parts [aria-pressed="true"]'); if (pb) pb.focus(); });
  top.appendChild(seg);
  box.appendChild(top);

  if (adjPart === "bg") {
    box.appendChild(swatches("Background colour", d.bg.colour, (c) => change((own) => { own.bg.colour = c; })));
    if (d.bg.type === "bands") box.appendChild(swatches("Second colour", d.bg.colour2, (c) => change((own) => { own.bg.colour2 = c; })));
    const two = Object.assign(document.createElement("div"), { className: "seg" });
    two.setAttribute("role", "group");
    two.setAttribute("aria-label", "Background kind");
    segment(two, [["colour", "One colour"], ["bands", "Two colours"]], d.bg.type === "bands" ? "bands" : "colour", (v) => change((own) => { own.bg.type = v; }, true), () => d.bg.type === "image" || d.bg.type === "gif");
    if (d.bg.type === "colour" || d.bg.type === "bands") box.appendChild(field("Kind", two));
  } else {
    const i = Number(adjPart), l = d.layers[i];
    if (l.type === "text") {
      const tin = Object.assign(document.createElement("input"), { type: "text", id: "ed-adj-text", value: l.text, maxLength: 24 });
      tin.addEventListener("input", () => change((own) => { own.layers[i].text = tin.value; }));
      const f = Object.assign(document.createElement("div"), { className: "field" });
      f.append(Object.assign(document.createElement("label"), { htmlFor: "ed-adj-text", textContent: "Text" }), tin);
      box.appendChild(f);
    }
    if (l.type === "text" || l.type === "icon") box.appendChild(swatches(l.type === "text" ? "Text colour" : "Icon colour", l.colour, (c) => change((own) => { own.layers[i].colour = c; })));
    if (l.type === "icon") box.appendChild(Object.assign(document.createElement("p"), { className: "help", textContent: `${iconLabel(l.glyph)}. To use another icon, pick it in the Icon or picture list${ownDesign(draft.states[stSel]) && ownDesign(draft.states[stSel]).fromIcon ? "" : " (that makes a new icon design), or open Edit"}.` }));
    if (l.type === "gif") {
      const sel = Object.assign(document.createElement("select"), { id: "ed-adj-gif" });
      GIFS.forEach((g) => sel.appendChild(Object.assign(document.createElement("option"), { value: g.id, textContent: g.name, selected: g.id === l.gif })));
      sel.addEventListener("change", () => change((own) => { own.layers[i].gif = sel.value; }));
      const f = Object.assign(document.createElement("div"), { className: "field" });
      f.append(Object.assign(document.createElement("label"), { htmlFor: "ed-adj-gif", textContent: "GIF" }), sel);
      box.appendChild(f);
    }
    const sliders = Object.assign(document.createElement("div"), { className: "adj-sliders" });
    if (l.type === "text") sliders.appendChild(slide("Size", 5, 90, l.size, (own, v) => { own.layers[i].size = v; }));
    else sliders.appendChild(slide("Size", 5, 100, l.w, (own, v) => { own.layers[i].w = v; }));
    sliders.append(
      slide("Across", 0, 100, l.x, (own, v) => { own.layers[i].x = v; }),
      slide("Down", 0, 100, l.y, (own, v) => { own.layers[i].y = v; })
    );
    box.appendChild(sliders);
    const row = Object.assign(document.createElement("div"), { className: "row" });
    const hide = Object.assign(document.createElement("button"), { type: "button", className: "btn sm", textContent: l.hidden ? "Show it" : "Hide it" });
    hide.setAttribute("aria-pressed", String(!!l.hidden));
    hide.addEventListener("click", () => change((own) => { own.layers[i].hidden = !own.layers[i].hidden; }, true));
    const del = Object.assign(document.createElement("button"), { type: "button", className: "btn sm", textContent: "Remove" });
    del.addEventListener("click", () => { const name = parts.find(([v]) => v === adjPart)[1]; adjPart = "bg"; change((own) => { own.layers.splice(i, 1); }, true); say(`${name} removed from this button's design`); });
    row.append(hide, del);
    box.appendChild(row);
  }
  // Add text or a GIF
  const add = Object.assign(document.createElement("div"), { className: "row adj-add" });
  const addText = Object.assign(document.createElement("button"), { type: "button", className: "btn sm", textContent: "+ Text" });
  addText.addEventListener("click", () => { change((own) => { own.layers.push(TX("TEXT", { x: 50, y: 20, size: 18, colour: textOn(asHex(own.bg.colour)) })); adjPart = String(own.layers.length - 1); }, true); say("Text added. Change what it says below."); });
  const addGif = Object.assign(document.createElement("button"), { type: "button", className: "btn sm", textContent: "+ GIF" });
  addGif.disabled = !GIFS.length;
  addGif.addEventListener("click", () => { change((own) => { own.layers.push(L("gif", { gif: GIFS[0].id, x: 50, y: 50, w: 40 })); adjPart = String(own.layers.length - 1); }, true); say("GIF added. Pick which one below."); });
  add.append(addText, addGif);
  box.appendChild(add);
}
