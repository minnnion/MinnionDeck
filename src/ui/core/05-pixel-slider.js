// ===== Pixel slider =====
// A slider drawn in the app's own style: notched track, stepped fill, a chunky grip and a value tag while you drag.
let pxsSeq = 0;
function pxSlider({ label, min, max, step = 1, value, unit = "", onInput, onChange }) {
  const id = `pxs${++pxsSeq}`;
  const w = document.createElement("div");
  w.className = "pxs";
  w.innerHTML = `<span class="pxs-lab" id="${id}-l"></span><div class="pxs-track" id="${id}" role="slider" tabindex="0" aria-labelledby="${id}-l" aria-valuemin="${min}" aria-valuemax="${max}"><span class="pxs-fill"></span><span class="pxs-thumb"><i></i><b class="pxs-tip" aria-hidden="true"></b></span></div><output class="pxs-out" for="${id}"></output>`;
  w.querySelector(".pxs-lab").textContent = label;
  const tr = w.querySelector(".pxs-track"), fill = w.querySelector(".pxs-fill"), th = w.querySelector(".pxs-thumb"), out = w.querySelector(".pxs-out"), tip = w.querySelector(".pxs-tip");
  let v = value;
  const fmt = (x) => `${x}${unit}`;
  const paint = () => {
    const pct = ((v - min) / (max - min)) * 100;
    fill.style.width = `${pct}%`;
    th.style.left = `${pct}%`;
    out.textContent = tip.textContent = fmt(v);
    tr.setAttribute("aria-valuenow", String(v));
    tr.setAttribute("aria-valuetext", fmt(v));
  };
  const set = (x) => {
    const n = Math.min(max, Math.max(min, Math.round((x - min) / step) * step + min));
    const r = Math.round(n * 1000) / 1000;
    if (r === v) return false;
    v = r; paint();
    if (onInput) onInput(v);
    return true;
  };
  tr.addEventListener("pointerdown", (e) => {
    e.preventDefault();
    tr.focus({ preventScroll: true });
    tr.setPointerCapture(e.pointerId);
    w.classList.add("drag");
    const at = (ev) => { const r = tr.getBoundingClientRect(); set(min + ((ev.clientX - r.left) / r.width) * (max - min)); };
    at(e);
    const mv = (ev) => at(ev);
    const up = () => { tr.removeEventListener("pointermove", mv); tr.removeEventListener("pointerup", up); tr.removeEventListener("pointercancel", up); w.classList.remove("drag"); if (onChange) onChange(v); };
    tr.addEventListener("pointermove", mv); tr.addEventListener("pointerup", up); tr.addEventListener("pointercancel", up);
  });
  tr.addEventListener("keydown", (e) => {
    const big = Math.max(step, (max - min) / 10);
    const k = { ArrowRight: step, ArrowUp: step, ArrowLeft: -step, ArrowDown: -step, PageUp: big, PageDown: -big }[e.key];
    let changed = false;
    if (k !== undefined) changed = set(v + (e.shiftKey ? k * 10 : k));
    else if (e.key === "Home") changed = set(min);
    else if (e.key === "End") changed = set(max);
    else return;
    e.preventDefault();
    if (changed && onChange) onChange(v);
  });
  paint();
  return w;
}

