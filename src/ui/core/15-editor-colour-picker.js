// ----- Colour picker: presets, recent colours and a custom hue / shade picker -----
const hexToHsv = (hex) => {
  const [r, g, b] = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
  const mx = Math.max(r, g, b), mn = Math.min(r, g, b), d = mx - mn;
  let h = 0;
  if (d) h = mx === r ? ((g - b) / d) % 6 : mx === g ? (b - r) / d + 2 : (r - g) / d + 4;
  return { h: Math.round((h * 60 + 360) % 360), s: mx ? d / mx : 0, v: mx };
};
const hsvToHex = ({ h, s, v }) => {
  const f = (n) => { const k = (n + h / 60) % 6; return v - v * s * Math.max(0, Math.min(k, 4 - k, 1)); };
  return "#" + [f(5), f(3), f(1)].map((x) => Math.round(x * 255).toString(16).padStart(2, "0")).join("");
};
const TOKEN_HEX = { tile: "#ffffff", mint: "#5fd0a8", sky: "#7cc4ff", lilac: "#b79cff", peach: "#ffb38a", sun: "#ffc93c", coral: "#ff5a5f" };
// A colour as a #hex. Only a real hex is passed through (an imported design could carry anything in
// this field, and the result is interpolated into gradients/styles); tokens map to their hex.
const asHex = (bg) => (typeof bg === "string" && /^#[0-9a-f]{3,8}$/i.test(bg) ? bg : (typeof bg === "string" && TOKEN_HEX[bg]) || "#ffffff");
let customOpen = false;
function colourPicker(box, st) {
  box.innerHTML = "";
  const isPreset = COLORS.some(([v]) => v === st.bg);
  if (!isPreset) customOpen = true;
  let hsv = hexToHsv(asHex(st.bg));
  const presets = document.createElement("div");
  presets.className = "presets";
  presets.setAttribute("role", "group");
  presets.setAttribute("aria-label", "Preset colours");
  const swatch = (v, name) => {
    const b = document.createElement("button");
    b.type = "button";
    b.style.background = bgCss(v);
    b.title = name;
    b.setAttribute("aria-label", name);
    b.setAttribute("aria-pressed", String(v === st.bg));
    b.addEventListener("click", () => { st.bg = v; customOpen = false; renderStates(); renderStateCard(); flipPreview(); refreshAll(); $("ed-bg").querySelector(`[aria-label="${name}"]`).focus(); });
    return b;
  };
  COLORS.forEach(([v, name]) => presets.appendChild(swatch(v, name)));
  const custom = document.createElement("button");
  custom.type = "button";
  custom.className = "custom-btn";
  custom.setAttribute("aria-expanded", String(customOpen));
  custom.setAttribute("aria-label", "Custom colour");
  custom.title = "Custom colour";
  custom.textContent = "+";
  custom.addEventListener("click", () => { customOpen = !customOpen; colourPicker(box, st); $("ed-bg").querySelector(".custom-btn").focus(); });
  presets.appendChild(custom);
  box.appendChild(presets);
  if (recentColors.length) {
    const rec = document.createElement("div");
    rec.className = "presets recent";
    rec.setAttribute("role", "group");
    rec.setAttribute("aria-label", "Recent colours");
    recentColors.forEach((hex) => rec.appendChild(swatch(hex, `Recent ${hex.toUpperCase()}`)));
    const cap = document.createElement("span");
    cap.className = "cp-note";
    cap.textContent = "Recent";
    box.append(cap, rec);
  }
  if (!customOpen) return;
  const panel = document.createElement("div");
  panel.className = "cp-panel";
  const sv = document.createElement("div");
  sv.className = "sv";
  sv.tabIndex = 0;
  sv.setAttribute("role", "slider");
  sv.setAttribute("aria-label", "Shade. Left and right change how strong the colour is, up and down change how light it is.");
  const knob = document.createElement("span");
  knob.className = "knob";
  sv.appendChild(knob);
  const hue = document.createElement("input");
  hue.type = "range"; hue.min = "0"; hue.max = "359"; hue.className = "hue";
  hue.setAttribute("aria-label", "Hue");
  const row = document.createElement("div");
  row.className = "cp-row";
  const prev = document.createElement("span");
  prev.className = "cp-prev";
  prev.textContent = "Aa";
  prev.setAttribute("aria-hidden", "true");
  const hex = document.createElement("input");
  hex.className = "hex";
  hex.maxLength = 7;
  hex.spellcheck = false;
  hex.setAttribute("aria-label", "Colour code");
  const note = document.createElement("span");
  note.className = "cp-note";
  note.setAttribute("aria-live", "polite");
  row.append(prev, hex, note);
  panel.append(sv, hue, row);
  box.appendChild(panel);
  const paint = (fromHex) => {
    const h = hsvToHex(hsv);
    sv.style.setProperty("--h", hsv.h);
    knob.style.left = `${hsv.s * 100}%`;
    knob.style.top = `${(1 - hsv.v) * 100}%`;
    hue.value = String(hsv.h);
    if (!fromHex) hex.value = h.toUpperCase();
    prev.style.background = h;
    prev.style.color = textOn(h);
    sv.setAttribute("aria-valuetext", `${Math.round(hsv.s * 100)}% strong, ${Math.round(hsv.v * 100)}% light`);
    note.textContent = `${textOn(h) === "#ffffff" ? "White" : "Dark"} text, contrast ${contrastOn(h).toFixed(1)}:1`;
  };
  const apply = (fromHex) => {
    st.bg = hsvToHex(hsv);
    paint(fromHex);
    presets.querySelectorAll("button[aria-pressed]").forEach((b) => b.setAttribute("aria-pressed", "false"));
    renderStates();
    refreshAll();
  };
  const remember = () => {
    const h = st.bg;
    if (!h.startsWith("#") || COLORS.some(([v]) => v === h)) return;
    const i = recentColors.indexOf(h);
    if (i >= 0) recentColors.splice(i, 1);
    recentColors.unshift(h);
    recentColors.length = Math.min(recentColors.length, 8);
  };
  const fromPoint = (ev) => {
    const r = sv.getBoundingClientRect();
    hsv.s = Math.max(0, Math.min(1, (ev.clientX - r.left) / r.width));
    hsv.v = 1 - Math.max(0, Math.min(1, (ev.clientY - r.top) / r.height));
    apply();
  };
  sv.addEventListener("pointerdown", (e) => {
    e.preventDefault();
    sv.focus();
    sv.setPointerCapture(e.pointerId);
    fromPoint(e);
    const mv = (ev) => fromPoint(ev);
    const up = () => { sv.removeEventListener("pointermove", mv); sv.removeEventListener("pointerup", up); remember(); flipPreview(); };
    sv.addEventListener("pointermove", mv);
    sv.addEventListener("pointerup", up);
  });
  sv.addEventListener("keydown", (e) => {
    const step = e.shiftKey ? 0.1 : 0.02;
    if (e.key === "ArrowRight") hsv.s = Math.min(1, hsv.s + step);
    else if (e.key === "ArrowLeft") hsv.s = Math.max(0, hsv.s - step);
    else if (e.key === "ArrowUp") hsv.v = Math.min(1, hsv.v + step);
    else if (e.key === "ArrowDown") hsv.v = Math.max(0, hsv.v - step);
    else return;
    e.preventDefault();
    apply();
  });
  sv.addEventListener("keyup", (e) => { if (e.key.startsWith("Arrow")) remember(); });
  hue.addEventListener("input", () => { hsv.h = Number(hue.value); if (hsv.s < 0.05) hsv.s = 0.6; if (hsv.v < 0.05) hsv.v = 0.9; apply(); });
  hue.addEventListener("change", remember);
  hex.addEventListener("input", () => {
    let v = hex.value.trim();
    if (!v.startsWith("#")) v = `#${v}`;
    const ok = /^#[0-9a-fA-F]{6}$/.test(v);
    hex.setAttribute("aria-invalid", String(!ok && v.length >= 7));
    if (ok) { hsv = hexToHsv(v.toLowerCase()); apply(true); }
  });
  hex.addEventListener("change", remember);
  paint();
}
function swatches(el, value, onPick) {
  el.innerHTML = "";
  COLORS.forEach(([tok, name]) => {
    const b = document.createElement("button");
    b.type = "button";
    b.style.background = `var(--${tok})`;
    b.setAttribute("aria-label", name);
    b.setAttribute("aria-pressed", String(tok === value));
    b.addEventListener("click", () => onPick(tok));
    el.appendChild(b);
  });
}

function renderEditor() {
  const hasDraft = !!draft;
  $("ed-empty").hidden = hasDraft || !!findTile("live");
  edForm.hidden = !hasDraft;
  if (!hasDraft) { $("editor-h").textContent = "Edit button"; refreshStatus(); if (!$("ed-modal").hidden) hideEditor(); return; }
  if (!draft.anim) draft.anim = defaultAnim();
  $("editor-h").textContent = isNew ? "New button" : "Edit button";
  $("ed-save").textContent = isNew ? "Add button" : "Save button";
  $("ed-cancel").textContent = isNew ? "Discard button" : "Undo changes";
  edName.value = draft.label;
  renderLook();
  renderMode();
  renderSequence();
  renderActionArea();
  renderDelete();
  refreshAll();
}

function renderLook() {
  renderAnim();
  renderGif();
  renderStates();
  renderStateCard();
  // Text only: the name (or a plugin's live value, such as an altitude) large in the middle, no icon
  segment($("ed-shape"), [["full", "Icon and name"], ["icon", "Icon only"], ["text", "Text only"]], draft.style.shape, (v) => { draft.style.shape = v; renderLook(); flipPreview(); refreshAll(); });
  segment($("ed-textpos"), [["bottom", "Below icon"], ["top", "Above icon"]], draft.style.textPos, (v) => { draft.style.textPos = v; renderLook(); flipPreview(); refreshAll(); }, () => draft.style.shape !== "full");
}

