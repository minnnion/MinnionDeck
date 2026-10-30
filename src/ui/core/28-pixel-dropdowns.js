// ===== Pixel dropdowns =====
// Every <select> in the app is replaced on screen by a pixel dropdown. The real select stays underneath,
// so its value, its change events and the code that reads it all keep working.
const SEL_VALUE = Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value");
let pxSelClose = null, pxSelPlace = null, pxSelSeq = 0;
function enhanceSelect(sel) {
  if (sel._px) return;
  sel._px = true;
  const id = `pxsel${++pxSelSeq}`;
  const wrap = document.createElement("span");
  wrap.className = sel.style.width === "auto" ? "pxsel inline" : "pxsel";
  sel.parentNode.insertBefore(wrap, sel);
  wrap.appendChild(sel);
  sel.tabIndex = -1;
  sel.setAttribute("aria-hidden", "true");
  sel.classList.add("pxsel-native");
  const b = document.createElement("button");
  b.type = "button";
  b.className = "pxsel-btn";
  b.id = `${id}-b`;
  b.setAttribute("aria-haspopup", "listbox");
  b.setAttribute("aria-expanded", "false");
  b.setAttribute("aria-controls", `${id}-l`);
  b.innerHTML = `<span class="pxsel-txt"></span><span class="pxsel-caret" aria-hidden="true"></span>`;
  const list = document.createElement("div");
  list.className = "pxsel-list";
  list.id = `${id}-l`;
  list.setAttribute("role", "listbox");
  list.tabIndex = -1;
  list.hidden = true;
  wrap.append(b, list);
  const lab = sel.id && document.querySelector(`label[for="${sel.id}"]`);
  if (lab) { if (!lab.id) lab.id = `${id}-lab`; lab.addEventListener("click", (e) => { e.preventDefault(); b.focus(); }); }
  const named = () => lab ? lab.id : sel.getAttribute("aria-labelledby");
  const sync = () => {
    const o = sel.options[sel.selectedIndex];
    const t = o ? o.textContent : "";
    b.querySelector(".pxsel-txt").textContent = t;
    if (named()) b.setAttribute("aria-labelledby", `${named()} ${b.id}`);
    else if (sel.getAttribute("aria-label")) b.setAttribute("aria-label", `${sel.getAttribute("aria-label")}: ${t}`);
    if (sel.getAttribute("aria-describedby")) b.setAttribute("aria-describedby", sel.getAttribute("aria-describedby"));
    list.setAttribute("aria-labelledby", named() || b.id);
    b.disabled = sel.disabled;
  };
  Object.defineProperty(sel, "value", { configurable: true, get() { return SEL_VALUE.get.call(sel); }, set(v) { SEL_VALUE.set.call(sel, v); sync(); } });
  sel.focus = () => b.focus();
  new MutationObserver(sync).observe(sel, { childList: true, subtree: true, attributes: true, attributeFilter: ["disabled", "aria-label", "aria-describedby"] });
  sel.addEventListener("change", sync);
  let active = 0, opts = [];
  const setActive = (i) => {
    if (!opts.length) return;
    active = Math.max(0, Math.min(opts.length - 1, i));
    opts.forEach((o, j) => o.classList.toggle("active", j === active));
    list.setAttribute("aria-activedescendant", opts[active].id);
    const o = opts[active];
    if (o.offsetTop < list.scrollTop) list.scrollTop = o.offsetTop - 4;
    else if (o.offsetTop + o.offsetHeight > list.scrollTop + list.clientHeight) list.scrollTop = o.offsetTop + o.offsetHeight - list.clientHeight + 4;
  };
  const close = (refocus) => {
    if (list.hidden || list.classList.contains("pop-out")) return;
    hidePop(list);
    b.setAttribute("aria-expanded", "false");
    if (pxSelClose === close) { pxSelClose = null; pxSelPlace = null; }
    if (refocus) b.focus();
  };
  const choose = (i) => {
    const o = sel.options[i];
    if (!o || o.disabled) return;
    const changed = sel.selectedIndex !== i;
    sel.selectedIndex = i;
    sync();
    close(true);
    if (changed) sel.dispatchEvent(new Event("change", { bubbles: true }));
  };
  const open = () => {
    if (b.disabled) return;
    if (pxSelClose) pxSelClose(false);
    list.innerHTML = "";
    opts = [...sel.options].map((o, i) => {
      const d = document.createElement("div");
      d.className = "pxsel-opt";
      d.id = `${id}-o${i}`;
      d.setAttribute("role", "option");
      d.setAttribute("aria-selected", String(i === sel.selectedIndex));
      if (o.disabled) d.setAttribute("aria-disabled", "true");
      d.textContent = o.textContent;
      d.addEventListener("pointerdown", (e) => e.preventDefault());
      d.addEventListener("click", () => choose(i));
      d.addEventListener("mousemove", () => { if (active !== i) setActive(i); });
      list.appendChild(d);
      return d;
    });
    showPop(list);
    place();
    b.setAttribute("aria-expanded", "true");
    setActive(Math.max(0, sel.selectedIndex));
    list.focus({ preventScroll: true });
    pxSelClose = close;
    pxSelPlace = place;
  };
  // Fixed to the screen so no scrolling panel can cut it off; opens upwards when there is no room below
  const place = () => {
    const r = b.getBoundingClientRect();
    if (r.bottom < 0 || r.top > window.innerHeight) { close(false); return; }
    const below = window.innerHeight - r.bottom - 12, above = r.top - 12;
    const up = below < 180 && above > below;
    const room = Math.max(120, Math.min(300, up ? above : below));
    list.style.minWidth = `${r.width}px`;
    list.style.maxWidth = `${Math.max(r.width, Math.min(420, window.innerWidth - r.left - 12))}px`;
    list.style.maxHeight = `${room}px`;
    list.style.left = `${Math.max(8, Math.min(r.left, window.innerWidth - list.offsetWidth - 12))}px`;
    list.style.top = up ? `${Math.max(8, r.top - Math.min(list.scrollHeight + 8, room) - 6)}px` : `${r.bottom + 6}px`;
  };
  b.addEventListener("click", () => (list.hidden ? open() : close(true)));
  b.addEventListener("keydown", (e) => { if (["ArrowDown", "ArrowUp", "Enter", " "].includes(e.key)) { e.preventDefault(); open(); } });
  let typed = "", typedAt = 0;
  list.addEventListener("keydown", (e) => {
    if (e.key === "ArrowDown") setActive(active + 1);
    else if (e.key === "ArrowUp") setActive(active - 1);
    else if (e.key === "Home") setActive(0);
    else if (e.key === "End") setActive(opts.length - 1);
    else if (e.key === "PageDown") setActive(active + 8);
    else if (e.key === "PageUp") setActive(active - 8);
    else if (e.key === "Enter" || e.key === " ") choose(active);
    else if (e.key === "Escape") { e.stopPropagation(); close(true); }
    else if (e.key === "Tab") { close(false); return; }
    else if (e.key.length === 1) {
      const now = Date.now();
      typed = now - typedAt > 700 ? e.key.toLowerCase() : typed + e.key.toLowerCase();
      typedAt = now;
      const i = opts.findIndex((o) => o.textContent.toLowerCase().startsWith(typed));
      if (i >= 0) setActive(i);
    } else return;
    e.preventDefault();
  });
  sync();
}
document.addEventListener("pointerdown", (e) => { if (pxSelClose && !e.target.closest(".pxsel, .pxsel-list")) pxSelClose(false); });
window.addEventListener("scroll", (e) => { if (pxSelPlace && !(e.target.closest && e.target.closest(".pxsel-list"))) pxSelPlace(); }, true);
window.addEventListener("resize", () => { if (pxSelClose) pxSelClose(false); });
document.querySelectorAll("select").forEach(enhanceSelect);
new MutationObserver((muts) => muts.forEach((m) => m.addedNodes.forEach((n) => {
  if (n.nodeType !== 1) return;
  if (n.tagName === "SELECT") enhanceSelect(n);
  else n.querySelectorAll("select").forEach(enhanceSelect);
}))).observe(document.body, { childList: true, subtree: true });

