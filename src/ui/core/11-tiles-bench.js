// ===== Tiles =====
// What a plugin shows on a button right now (Host::set_title / set_icon), over its saved look.
// Not saved: it lasts until the plugin clears it or Minniondeck restarts.
const LIVE = {};
function withLive(t, st, gp, size) {
  const live = LIVE[t.id];
  if (live && live.icon) gp.icon = `<img class="live-ico" src="${live.icon}" alt="" width="${size}" height="${size}">`;
  return live && typeof live.title === "string" ? live.title : st.text || t.label;
}
function fillTile(el, t, idx = t.cur || 0) {
  // A screen (a live map, a flight plan, radio messages) fills the whole button (26-live-map.js)
  const isMap = !!screenDataOf(t);
  el.classList.toggle("has-map", isMap);
  el.classList.toggle("has-screen", isMap);
  if (isMap) {
    ["has-gif", "has-wdg", "has-img", "has-dsg", "icon-only", "text-only", "text-top"].forEach((c) => el.classList.remove(c));
    el.style.background = "";
    el.innerHTML = screenHTML(t.id);
    requestAnimationFrame(() => drawMaps(t.id));
    return;
  }
  const st = t.states[Math.min(idx, t.states.length - 1)];
  const multi = t.states.length > 1;
  // No plugin minnion on the button itself: it sat on every plugin button's face. Who runs a
  // button is in its label (tileLabel) and in the inspector.
  const faceHTML = "";
  const stepsHTML = t.mode === "sequence" ? '<span class="chip steps"></span>' : "";
  const stateHTML = multi ? '<span class="chip state"></span>' : "";
  const iconOnly = t.style.shape === "icon";
  const gp = gifParts(st, iconOnly ? 40 : 24);
  const text = withLive(t, st, gp, iconOnly ? 40 : 24);
  const sb = stateBg(st);
  const liveTitle = LIVE[t.id] && typeof LIVE[t.id].title === "string" ? LIVE[t.id].title : "";
  el.innerHTML = `${sb.design ? designHTML(sb.design, t.w, t.h, liveTitle) : sb.under}${gp.bg}<span class="top">${gp.icon}<span class="badges">${stepsHTML}${stateHTML}${faceHTML}</span></span>${gp.wdg}<span class="lbl"></span>`;
  el.classList.toggle("has-gif", !!gp.bg);
  el.classList.toggle("has-wdg", !!gp.wdg);
  el.classList.toggle("has-img", sb.pic);
  el.classList.toggle("has-dsg", !!sb.design);
  el.querySelector(".lbl").textContent = text;
  const s = el.querySelector(".steps"); if (s) s.textContent = `${t.steps.length} steps`;
  const g = el.querySelector(".state"); if (g) g.textContent = st.name;
  el.classList.toggle("icon-only", iconOnly);
  el.classList.toggle("text-only", t.style.shape === "text");
  el.classList.toggle("text-top", t.style.textPos === "top" && t.style.shape === "full");
  const fill = st.bg;
  el.style.background = sb.css;
  // Light accent fills keep dark text in both themes
  el.style.color = fill === "tile" ? "" : textOn(fill);
  knobAngle(el, t);
}
// A knob's picture turns as the knob does, all the way round and on (t._angle: degrees, kept for
// this session only; underscore fields aren't saved). Its first picture layer is the knob.
function knobAngle(el, t) {
  if (!t.knob) return;
  const k = el.querySelector(".dsg .dl-icon");
  if (!k) return;
  // Added to the layer's own transform (centring, then its turn): CSS's separate rotate property
  // is applied before that centring, which turned the knob round its corner
  if (!k.dataset.base) k.dataset.base = k.style.transform;
  k.style.transform = `${k.dataset.base} rotate(${t._angle || 0}deg)`;
}
function tileLabel(t) {
  const owner = tileOwner(t);
  const parts = [t.label, owner ? `run by ${owner}` : "built-in"];
  if (t.mode === "sequence") parts.push(`${t.steps.length} steps`);
  if (t.states.length > 1) parts.push(`state ${t.states[t.cur || 0].name}`);
  return parts.join(", ");
}

// ===== Bench =====
function renderSeat(i, hidden = false) {
  const seat = bench.querySelector(`[data-i="${i}"]`);
  const m = crew[i];
  seat.innerHTML = `<span style="visibility:${hidden ? "hidden" : "visible"}">${minnion({ c: pal(m.body), expr: m.expr, look: m.look, tall: m.tall, scale: 3 })}</span><small></small>`;
  seat.querySelector("small").textContent = m.name;
}
function renderBench() {
  bench.innerHTML = "";
  crew.forEach((m, i) => {
    const seat = document.createElement("div");
    seat.className = "seat";
    seat.dataset.i = i;
    bench.appendChild(seat);
    renderSeat(i);
  });
}
function mates(except, patch, hop = false) {
  crew.forEach((m, i) => {
    if (i === except) return;
    Object.assign(m, patch);
    renderSeat(i);
    bench.querySelector(`[data-i="${i}"]`).classList.toggle("hop", hop);
  });
}

