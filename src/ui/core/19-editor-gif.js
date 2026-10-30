// ----- GIF or widget, per state -----
let gpickOpen = false, gpickActive = null, gpickOpts = [];
const curGif = () => { const st = draft.states[stSel]; if (!st.gif) st.gif = GIF_NONE(); return st.gif; };
function gifSummary() {
  const used = draft.states.map((st, i) => [st, i]).filter(([st]) => gifOf(st));
  $("sum-gif").textContent = !used.length ? "None"
    : draft.states.length === 1 ? `${gifOf(used[0][0]).name}${gifOf(used[0][0]).kind === "gif" ? `, ${used[0][0].gif.place === "icon" ? "icon" : "background"}` : ""}`
    : used.map(([st]) => `${gifOf(st).name} on ${st.name}`).join(", ");
}
function renderGif() {
  const g = curGif(), def = GIF[g.id];
  const multi = draft.states.length > 1;
  $("ed-gif-state-field").hidden = !multi;
  if (multi) segment($("ed-gif-state"), draft.states.map((st, i) => [i, st.name]), stSel, (i) => { selectState(i); const b = $("ed-gif-state").querySelector('[aria-pressed="true"]'); if (b) b.focus(); });
  renderGifPicker();
  const isGif = !!def && def.kind === "gif";
  $("ed-gif-place-field").hidden = !isGif;
  $("ed-gif-speed-field").hidden = !isGif;
  const seg = (id, opts, val, fn) => segment($(id), opts, val, (v) => { fn(v); renderGif(); refreshAll(); flipPreview(); const b = $(id).querySelector('[aria-pressed="true"]'); if (b) b.focus(); });
  if (isGif) {
    seg("ed-gif-place", [["bg", "Background"], ["icon", "Icon"]], g.place, (v) => { g.place = v; });
    seg("ed-gif-speed", [["slow", "Slow"], ["normal", "Normal"], ["fast", "Fast"]], g.speed, (v) => { g.speed = v; });
  }
  const who = multi ? ` Only state ${stSel + 1}, ${draft.states[stSel].name}, shows it.` : "";
  $("ed-gif-help").textContent = !def ? `No GIF. Pick one to bring the button to life, like a widget on a phone home screen.${multi ? " Each state can have its own." : ""}`
    : def.kind === "widget" ? `${def.name} is a live widget. It updates by itself and takes the place of the icon.${who}`
    : g.place === "bg" ? `Plays behind the icon and name. The name gets a dark backing so it stays easy to read.${who}`
    : `Plays in the icon's place.${who}`;
  gifSummary();
  renderGifTools(def);
}
// Import, and Make on the website: GIFs are drawn on the Minniondeck website and come back as
// GIF files with Import
function renderGifTools(def) {
  const tools = $("ed-gif-tools");
  tools.innerHTML = "";
  const tb = (label, fn, aria) => { const b = Object.assign(document.createElement("button"), { type: "button", className: "btn sm", textContent: label }); if (aria) b.setAttribute("aria-label", aria); b.addEventListener("click", fn); return b; };
  const imp = tb("Import…", () => uploadGif((x) => { chooseGif(x.id); say(`${x.name} added and put on the button`); }), "Import a GIF file");
  const web = tb("Make on the website", () => openSite("gifs"), "Make a GIF on the Minniondeck website");
  tools.append(imp, web);
}
function gifMini(id) {
  const g = GIF[id];
  if (!g) return `<span class="gmini" aria-hidden="true">${glyph("cross", 14)}</span>`;
  if (g.kind === "widget") return `<span class="gmini" aria-hidden="true"><span class="wdg" data-w="${id}">${widgetFace(id)}</span></span>`;
  return `<span class="gmini" aria-hidden="true"><span class="gif-bg">${gifStrip(g)}</span></span>`;
}
function renderGifPicker() {
  const area = $("ed-gif-area");
  area.innerHTML = "";
  gpickOpen = false;
  const g = curGif(), def = GIF[g.id];
  const wrap = Object.assign(document.createElement("div"), { className: "pick", id: "ed-gpick" });
  const btn = Object.assign(document.createElement("button"), { type: "button", className: "pick-btn", id: "ed-gpick-btn" });
  btn.setAttribute("aria-haspopup", "listbox");
  btn.setAttribute("aria-expanded", "false");
  btn.setAttribute("aria-labelledby", "cap-gif ed-gpick-btn");
  btn.innerHTML = `${gifMini(g.id)}<span class="pick-txt"><span></span><small></small></span>${glyph("caret", 16)}`;
  btn.querySelector(".pick-txt span").textContent = def ? def.name : "None";
  btn.querySelector(".pick-txt small").textContent = def ? `${def.kind === "widget" ? "Widget" : "GIF"} · ${def.desc}` : "No GIF or widget";
  const pop = Object.assign(document.createElement("div"), { className: "pick-pop", id: "ed-gpick-pop" });
  pop.hidden = true;
  const list = Object.assign(document.createElement("div"), { className: "picker", id: "ed-gifs", tabIndex: -1 });
  list.setAttribute("role", "listbox");
  list.setAttribute("aria-labelledby", "cap-gif");
  pop.appendChild(list);
  wrap.append(btn, pop);
  area.append(wrap);
  btn.addEventListener("click", () => (gpickOpen ? closeGPick(true) : openGPick()));
  btn.addEventListener("keydown", (e) => { if (e.key === "ArrowDown" || e.key === "ArrowUp") { e.preventDefault(); openGPick(); } });
  list.addEventListener("keydown", (e) => {
    const i = gpickOpts.findIndex((o) => o.dataset.id === gpickActive);
    if (e.key === "ArrowDown") setGActive(i + 1);
    else if (e.key === "ArrowUp") setGActive(i - 1);
    else if (e.key === "Home") setGActive(0);
    else if (e.key === "End") setGActive(gpickOpts.length - 1);
    else if (e.key === "Enter" || e.key === " ") chooseGif(gpickActive);
    else if (e.key === "Escape") { e.stopPropagation(); closeGPick(true); }
    else if (e.key === "Tab") { closeGPick(false); return; }
    else return;
    e.preventDefault();
  });
}
function buildGifList() {
  const list = $("ed-gifs");
  list.innerHTML = "";
  gpickOpts = [];
  const cur = curGif().id;
  // Each plugin's GIFs are listed under the plugin's name
  const fromPlugins = plugins.map((p) => [p.name, GIFS.filter((g) => g.pluginId === p.id)]).filter(([, items]) => items.length);
  [["Off", [{ id: "none", name: "None", desc: "Just the icon" }]], ["Widgets", WIDGETS], ["GIFs", GIFS.filter((g) => !g.pluginId)], ...fromPlugins].forEach(([gname, items], gi) => {
    const group = document.createElement("div");
    group.setAttribute("role", "group");
    group.setAttribute("aria-labelledby", `ggrp-${gi}`);
    group.appendChild(Object.assign(document.createElement("div"), { className: "grp", id: `ggrp-${gi}`, textContent: gname }));
    items.forEach((x) => {
      const o = Object.assign(document.createElement("div"), { className: "opt", id: `gf-${x.id.replace(/[^\w-]/g, "_")}` });
      o.dataset.id = x.id;
      o.setAttribute("role", "option");
      o.setAttribute("aria-selected", String(x.id === cur));
      o.innerHTML = `${gifMini(x.id)}<span class="opt-txt"><span></span><small></small></span>${x.id === cur ? '<span class="opt-check" aria-hidden="true">✓</span>' : ""}`;
      o.querySelector(".opt-txt span").textContent = x.name;
      o.querySelector(".opt-txt small").textContent = x.desc;
      o.addEventListener("click", () => chooseGif(x.id));
      o.addEventListener("mousemove", () => { if (gpickActive !== x.id) setGActive(gpickOpts.indexOf(o), false); });
      group.appendChild(o);
      gpickOpts.push(o);
    });
    list.appendChild(group);
  });
}
function setGActive(i, scroll = true) {
  const k = Math.max(0, Math.min(gpickOpts.length - 1, i));
  gpickActive = gpickOpts[k].dataset.id;
  gpickOpts.forEach((o, j) => o.classList.toggle("active", j === k));
  $("ed-gifs").setAttribute("aria-activedescendant", gpickOpts[k].id);
  if (!scroll) return;
  const list = $("ed-gifs"), o = gpickOpts[k];
  const lr = list.getBoundingClientRect(), orr = o.getBoundingClientRect(), head = 30;
  if (orr.top < lr.top + head) list.scrollTop -= lr.top + head - orr.top;
  else if (orr.bottom > lr.bottom) list.scrollTop += orr.bottom - lr.bottom;
}
function openGPick() {
  if (gpickOpen) return;
  closePick(false); closeIPick(false); closeAPick(false);
  gpickOpen = true;
  showPop($("ed-gpick-pop"));
  $("ed-gpick-btn").setAttribute("aria-expanded", "true");
  buildGifList();
  setGActive(Math.max(0, gpickOpts.findIndex((o) => o.dataset.id === curGif().id)));
  $("ed-gpick-pop").scrollIntoView({ block: "nearest" });
  $("ed-gifs").focus({ preventScroll: true });
}
function closeGPick(refocus) {
  if (!gpickOpen) return;
  gpickOpen = false;
  hidePop($("ed-gpick-pop"));
  $("ed-gpick-btn").setAttribute("aria-expanded", "false");
  if (refocus) $("ed-gpick-btn").focus();
}
function chooseGif(id) {
  if (!id) return;
  const g = curGif();
  g.id = id;
  closeGPick(false);
  renderGif();
  renderStates();
  refreshAll();
  flipPreview();
  $("ed-gpick-btn").focus();
}
document.addEventListener("pointerdown", (e) => { if (gpickOpen && !e.target.closest("#ed-gpick")) closeGPick(false); });

