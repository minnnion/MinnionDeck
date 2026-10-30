// ----- Icon dropdown -----
let ipickOpen = false, ipickActive = null, ipickOpts = [], iconSearch = "";
function renderIconPicker() {
  const area = $("ed-icon-area");
  area.innerHTML = "";
  ipickOpen = false;
  const wrap = Object.assign(document.createElement("div"), { className: "pick", id: "ed-ipick" });
  const btn = Object.assign(document.createElement("button"), { type: "button", className: "pick-btn", id: "ed-ipick-btn" });
  btn.setAttribute("aria-haspopup", "listbox");
  btn.setAttribute("aria-expanded", "false");
  btn.setAttribute("aria-labelledby", "cap-icon ed-ipick-btn");
  const pop = Object.assign(document.createElement("div"), { className: "pick-pop", id: "ed-ipick-pop" });
  pop.hidden = true;
  const search = Object.assign(document.createElement("input"), { id: "ed-isearch", type: "search", placeholder: `Search ${iconCount()} icons`, autocomplete: "off" });
  search.setAttribute("role", "combobox");
  search.setAttribute("aria-label", "Search icons");
  search.setAttribute("aria-autocomplete", "list");
  search.setAttribute("aria-controls", "ed-icons");
  search.setAttribute("aria-expanded", "false");
  const list = Object.assign(document.createElement("div"), { className: "picker", id: "ed-icons" });
  list.setAttribute("role", "listbox");
  list.setAttribute("aria-labelledby", "cap-icon");
  pop.append(search, list);
  wrap.append(btn, pop);
  area.appendChild(wrap);
  btn.addEventListener("click", () => (ipickOpen ? closeIPick(false) : openIPick()));
  btn.addEventListener("keydown", (e) => { if (e.key === "ArrowDown" || e.key === "ArrowUp") { e.preventDefault(); openIPick(); } });
  search.addEventListener("input", () => { iconSearch = search.value; ipickActive = null; buildIconList(); });
  search.addEventListener("keydown", (e) => {
    const i = ipickOpts.findIndex((o) => o.dataset.id === ipickActive);
    if (e.key === "ArrowRight") setIActive(i + 1);
    else if (e.key === "ArrowLeft") setIActive(i - 1);
    else if (e.key === "ArrowDown") setIActive(rowNeighbour(i, 1));
    else if (e.key === "ArrowUp") setIActive(rowNeighbour(i, -1));
    else if (e.key === "Enter") { if (ipickActive) chooseIcon(ipickActive); }
    else if (e.key === "Escape") { e.stopPropagation(); closeIPick(true); }
    else if (e.key === "Tab") { closeIPick(false); return; }
    else return;
    e.preventDefault();
  });
  paintIconBtn();
}
function paintIconBtn() {
  const btn = $("ed-ipick-btn");
  if (!btn) return;
  const id = draft.states[stSel].glyph;
  btn.innerHTML = `${iconChip(id)}<span class="pick-txt"><span></span><small></small></span>${glyph("caret", 16)}`;
  btn.querySelector(".pick-txt span").textContent = ICON_NAMES[id] || id;
  btn.querySelector(".pick-txt small").textContent = iconPathOf(id);
}
function buildIconList() {
  const list = $("ed-icons");
  const q = iconSearch.trim().toLowerCase();
  const curId = draft.states[stSel].glyph;
  list.innerHTML = "";
  ipickOpts = [];
  let k = 0;
  ICON_SETS.forEach((set, gi) => {
    const icons = set.icons.filter((id) => !q || (ICON_NAMES[id] || id).toLowerCase().includes(q) || id.includes(q) || set.name.toLowerCase().includes(q));
    if (!icons.length) return;
    const st = set.plugin ? pluginState(set.plugin) : null;
    const group = document.createElement("div");
    group.setAttribute("role", "group");
    group.setAttribute("aria-labelledby", `igrp-${gi}`);
    const head = Object.assign(document.createElement("div"), { className: "grp", id: `igrp-${gi}` });
    head.innerHTML = `${set.plugin ? minnion({ c: st === "dizzy" ? pal("coral", "var(--sun)") : pal(PLUGIN_BODY[set.plugin]), expr: st === "ready" ? "open" : st === "dizzy" ? "dizzy" : "sleepy", scale: 1 }) : ""}<span></span><small></small><span class="grp-path" aria-hidden="true"></span>`;
    head.querySelector("span").textContent = set.name;
    head.querySelector("small").textContent = `${icons.length} ${icons.length === 1 ? "icon" : "icons"}${st === "missing" ? ", not hired yet" : ""}`;
    head.querySelector(".grp-path").textContent = set.folder;
    const cells = Object.assign(document.createElement("div"), { className: "icon-cells" });
    icons.forEach((id) => {
      const o = Object.assign(document.createElement("div"), { className: "icell", id: `ic-${id.replace(/[^\w-]/g, "_")}` });
      o.dataset.id = id;
      o.setAttribute("role", "option");
      o.setAttribute("aria-selected", String(id === curId));
      if (st === "missing") o.setAttribute("aria-disabled", "true");
      o.title = iconPathOf(id);
      o.style.setProperty("--d", `${Math.min(k++ * 10, 300)}ms`);
      o.innerHTML = `${iconChip(id)}<span></span>`;
      o.querySelector("span:last-child").textContent = ICON_NAMES[id] || id;
      o.addEventListener("pointerdown", (e) => e.preventDefault());
      o.addEventListener("click", () => chooseIcon(id));
      o.addEventListener("mousemove", () => { if (ipickActive !== id) setIActive(ipickOpts.indexOf(o), false); });
      cells.appendChild(o);
      ipickOpts.push(o);
    });
    group.append(head, cells);
    list.appendChild(group);
  });
  if (!ipickOpts.length) {
    list.appendChild(Object.assign(document.createElement("div"), { className: "none", textContent: `No icon matches "${iconSearch}". Try a shorter word.` }));
    ipickActive = null;
    $("ed-isearch").removeAttribute("aria-activedescendant");
    return;
  }
  const keep = ipickOpts.findIndex((o) => o.dataset.id === ipickActive);
  const cur = ipickOpts.findIndex((o) => o.dataset.id === curId);
  setIActive(keep >= 0 ? keep : q ? 0 : Math.max(0, cur));
}
// The option in the next or previous visual row, closest to the same column
function rowNeighbour(i, dir) {
  if (i < 0) return 0;
  const r = ipickOpts[i].getBoundingClientRect();
  const cands = ipickOpts.map((o, j) => [j, o.getBoundingClientRect()]).filter(([, b]) => (dir > 0 ? b.top > r.top + 4 : b.top < r.top - 4));
  if (!cands.length) return i;
  const rowTop = dir > 0 ? Math.min(...cands.map(([, b]) => b.top)) : Math.max(...cands.map(([, b]) => b.top));
  const row = cands.filter(([, b]) => Math.abs(b.top - rowTop) < 4);
  row.sort((a, b) => Math.abs(a[1].left - r.left) - Math.abs(b[1].left - r.left));
  return row[0][0];
}
function setIActive(i, scroll = true) {
  if (!ipickOpts.length) return;
  const k = Math.max(0, Math.min(ipickOpts.length - 1, i));
  ipickActive = ipickOpts[k].dataset.id;
  ipickOpts.forEach((o, j) => o.classList.toggle("active", j === k));
  $("ed-isearch").setAttribute("aria-activedescendant", ipickOpts[k].id);
  if (!scroll) return;
  const list = $("ed-icons"), o = ipickOpts[k];
  const lr = list.getBoundingClientRect(), orr = o.getBoundingClientRect(), head = 34;
  if (orr.top < lr.top + head) list.scrollTop -= lr.top + head - orr.top;
  else if (orr.bottom > lr.bottom) list.scrollTop += orr.bottom - lr.bottom;
}
function openIPick() {
  if (ipickOpen || !$("ed-ipick-btn")) return;
  closePick(false);
  closeAPick(false);
  closeGPick(false);
  ipickOpen = true;
  iconSearch = "";
  ipickActive = null;
  const search = $("ed-isearch"), pop = $("ed-ipick-pop");
  search.value = "";
  showPop(pop);
  $("ed-ipick-btn").setAttribute("aria-expanded", "true");
  search.setAttribute("aria-expanded", "true");
  buildIconList();
  pop.scrollIntoView({ block: "nearest" });
  search.focus({ preventScroll: true });
}
function closeIPick(refocus) {
  if (!ipickOpen) return;
  ipickOpen = false;
  hidePop($("ed-ipick-pop"));
  $("ed-ipick-btn").setAttribute("aria-expanded", "false");
  $("ed-isearch").setAttribute("aria-expanded", "false");
  $("ed-isearch").removeAttribute("aria-activedescendant");
  if (refocus) $("ed-ipick-btn").focus();
}
function chooseIcon(id) {
  const set = iconSet(id);
  if (set.plugin && pluginState(set.plugin) === "missing") return;
  draft.states[stSel].glyph = id;
  closeIPick(true);
  paintIconBtn();
  renderStates();
  flipPreview();
  refreshAll();
}
document.addEventListener("pointerdown", (e) => { if (ipickOpen && !e.target.closest("#ed-ipick")) closeIPick(false); });

