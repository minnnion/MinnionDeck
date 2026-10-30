// Dropdowns a plugin fills itself (a field with "dynamic": true). Choices are asked for once and
// kept until the plugin says they changed (Host::refresh_options) or plugins reload.
const DYN_OPTS = {};
function dynamicOptions(a, fd, step) {
  const key = `${a.id}|${fd.key}`;
  const got = DYN_OPTS[key];
  if (Array.isArray(got)) return got.slice();
  if (!got && pluginState(a.plugin) === "ready") {
    DYN_OPTS[key] = "loading";
    backend.call("plugin_options", { kind: a.id, field: fd.key, fields: step.f }, []).then((list) => {
      DYN_OPTS[key] = Array.isArray(list) ? list : [];
      if (draft && draft.steps[sel] && draft.steps[sel].type === a.id) renderFields();
    });
  }
  return [];
}
// The button becomes the states a plugin's choice brings: each with its name and its picture
// filling the button, the state's text under it (a design of its own, so the Image section can
// change it and the creator edit, move and scale it). The plugin switches between them
// (Host::set_state).
function applyChoiceStates(a, states) {
  const base = draft.states[0];
  draft.states = states.slice(0, 8).map((s, i) => {
    const st = { ...clone(base), name: s.name || `State ${i + 1}`, text: "", bg: "#2d3142", bgType: "colour", gif: GIF_NONE() };
    delete st.design;
    if (s.icon) {
      const d = pictureDesign(`plug:${a.pluginId}:${s.icon}`, Number(s.ar) || 1, s.text || "", `${draft.label || a.label} ${s.name || i + 1}`.slice(0, 40));
      st.bgType = "design";
      st.design = d.id;
    } else st.text = s.text || "";
    return st;
  });
  stSel = 0;
}
function renderFields() {
  const box = $("ed-fields");
  if (!box) return;
  box.innerHTML = "";
  const step = draft.steps[sel];
  const a = ACTION[step.type];
  if (!a.fields.length) {
    const p = document.createElement("p");
    p.className = "muted";
    p.textContent = a.ownStates ? `${a.label} needs no settings. ${a.plugin} switches it between ${a.dual.join(", ")}.` : a.dual ? `${a.label} needs no settings. It switches between ${a.dual[0]} and ${a.dual[1]} each time you press it.` : `${a.label} needs no settings.`;
    box.appendChild(p);
    return;
  }
  a.fields.forEach((fd) => {
    const key = `${sel}:${fd.key}`;
    const id = `f-${sel}-${fd.key}`;
    const wrap = document.createElement("div");
    wrap.className = fd.drop ? "field drop-field" : "field";
    const label = document.createElement("label");
    label.htmlFor = id;
    label.textContent = fd.required ? fd.label : `${fd.label} (optional)`;
    let ctrl;
    if (fd.kind === "select") {
      ctrl = document.createElement("select");
      // Go to page: the profile's pages and folders ("Fenix / Overhead / Fuel"), from the app
      const opts = fd.pages ? (window.pageChoices ? window.pageChoices() : []) : fd.dynamic ? dynamicOptions(a, fd, step) : fd.options.map((o) => ({ value: o, label: o }));
      if (fd.pages && !step.f[fd.key] && opts.length) step.f[fd.key] = opts[0].value;
      // A saved choice the list doesn't have (yet) stays, so opening the editor never changes it
      if (step.f[fd.key] && !opts.some((o) => o.value === step.f[fd.key])) opts.unshift({ value: step.f[fd.key], label: step.f[fd.key] });
      opts.forEach((o) => { const op = document.createElement("option"); op.value = o.value; op.textContent = o.label; ctrl.appendChild(op); });
      ctrl.value = step.f[fd.key];
      ctrl.addEventListener("change", () => {
        step.f[fd.key] = ctrl.value;
        touched.add(key);
        // A choice that brings its own looks (a Fenix control's lamp off and on, say)
        const o = opts.find((x) => x.value === ctrl.value);
        if (o && Array.isArray(o.states) && o.states.length && draft.mode === "single") {
          applyChoiceStates(a, o.states);
          renderStates(); renderStateCard(); renderLook();
        }
        refreshAll();
      });
    } else {
      ctrl = document.createElement("input");
      ctrl.type = fd.kind === "number" ? "number" : fd.kind === "url" ? "url" : "text";
      if (fd.kind === "number") { ctrl.min = fd.min; ctrl.max = fd.max; ctrl.inputMode = "numeric"; }
      if (fd.max && fd.kind !== "number") ctrl.maxLength = fd.max;
      if (fd.placeholder) ctrl.placeholder = fd.placeholder;
      ctrl.value = step.f[fd.key];
      if (fd.kind === "keys") {
        ctrl.placeholder = "Click here, then press keys";
        ctrl.addEventListener("keydown", (e) => {
          if (e.key === "Tab" && !e.ctrlKey && !e.altKey && !e.metaKey) return;
          e.preventDefault();
          const put = (v) => { ctrl.value = v; step.f[fd.key] = v; touched.add(key); refreshAll(); };
          if ((e.key === "Backspace" || e.key === "Delete") && !e.ctrlKey && !e.altKey && !e.shiftKey) { put(""); return; }
          const mods = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Win"].filter(Boolean);
          if (["Control", "Alt", "Shift", "Meta"].includes(e.key)) { ctrl.value = `${mods.join("+")}+`; return; }
          const k = e.key === " " ? "Space" : e.key.length === 1 ? e.key.toUpperCase() : e.key;
          put([...mods, k].join("+"));
        });
        ctrl.addEventListener("blur", () => { ctrl.value = step.f[fd.key]; touched.add(key); refreshAll(); });
      } else {
        ctrl.addEventListener("input", () => { step.f[fd.key] = ctrl.value; refreshAll(); });
        ctrl.addEventListener("blur", () => { touched.add(key); refreshAll(); });
      }
    }
    ctrl.id = id;
    ctrl.dataset.where = key;
    const err = document.createElement("span");
    err.className = "err";
    err.id = `err-${id}`;
    const help = fd.help ? Object.assign(document.createElement("span"), { className: "help", id: `help-${id}`, textContent: fd.help }) : null;
    ctrl.setAttribute("aria-describedby", [err.id, help && help.id].filter(Boolean).join(" "));
    wrap.append(label);
    if (fd.unit) {
      const u = document.createElement("div");
      u.className = "unit";
      const unit = document.createElement("span");
      unit.textContent = fd.unit;
      u.append(ctrl, unit);
      wrap.append(u);
    } else wrap.append(ctrl);
    if (help) wrap.append(help);
    wrap.append(err);
    box.appendChild(wrap);
  });
}

// Dropping a program on the Edit window: anywhere on it fills the Program box (the box lights up
// while a file is dragged over the window). Shortcuts (.lnk), web shortcuts such as Steam games
// (.url) and scripts work too; Windows opens them the same way as a double-click.
const PROGRAM_EXT = /\.(exe|lnk|url|bat|cmd|com|msc|appref-ms)$/i;
backend.onFileDrop((type, paths) => {
  document.querySelectorAll(".drop-field.drop-on").forEach((e) => e.classList.remove("drop-on"));
  const open = draft && !$("ed-modal").hidden;
  const f = open && $("ed-fields") ? $("ed-fields").querySelector(".drop-field") : null;
  if (!f) {
    if (type === "drop" && paths.length) say(open ? "Pick Open a program under What it does, then drop it again." : "Open a button's Edit window and pick Open a program, then drop it there.");
    return;
  }
  if (type === "enter" || type === "over") { f.classList.add("drop-on"); return; }
  if (type !== "drop") return;
  const p = paths.find((x) => PROGRAM_EXT.test(x));
  if (!p) { say("That isn't a program. Drop an .exe file or a shortcut."); return; }
  const inp = f.querySelector("input");
  inp.value = p;
  inp.dispatchEvent(new Event("input"));
  touched.add(inp.dataset.where);
  const name = p.split(/[\\/]/).pop().replace(/\.[^.]+$/, "");
  // A button without a name takes the program's
  if (!String(draft.label || "").trim()) { draft.label = name; edName.value = name; }
  refreshAll();
  inp.focus();
  say(`Program set to ${name}`);
});

function renderWarn() {
  const box = $("ed-warn");
  if (!box) return;
  box.innerHTML = "";
  const p = pluginOf(draft.steps[sel]);
  if (!p) return;
  const st = pluginState(p);
  if (st === "ready") return;
  const w = document.createElement("div");
  w.className = "warn";
  const text = document.createElement("p");
  text.textContent = st === "missing"
    ? `${p} is not hired yet. Hire it to use this action.`
    : st === "dizzy"
      ? `${p} is stopped. You can still save, but this step will not run until ${p} is restarted.`
      : `${p} is still starting. It should be ready in a moment.`;
  w.appendChild(text);
  if (st !== "waking") {
    const go = document.createElement("button");
    go.type = "button";
    go.className = "btn link";
    go.textContent = st === "missing" ? `Hire it in Settings` : `Open plugin settings`;
    go.addEventListener("click", () => { if (window.openPluginSettings) window.openPluginSettings(); });
    w.appendChild(go);
  }
  box.appendChild(w);
}

function renderDelete() {
  const area = $("ed-delete-area");
  area.innerHTML = "";
  if (isNew) return;
  if (!confirmDelete) {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "btn";
    b.textContent = "Delete button";
    b.addEventListener("click", () => { confirmDelete = true; renderDelete(); area.querySelector("button").focus(); });
    area.appendChild(b);
    return;
  }
  const c = document.createElement("span");
  c.className = "confirm";
  const q = document.createElement("span");
  q.textContent = `Delete ${findTile(selectedId).label}?`;
  const yes = Object.assign(document.createElement("button"), { type: "button", className: "btn go", textContent: "Delete" });
  const no = Object.assign(document.createElement("button"), { type: "button", className: "btn", textContent: "Keep it" });
  yes.addEventListener("click", () => removeTile(selectedId, true));
  no.addEventListener("click", () => { confirmDelete = false; renderDelete(); });
  c.append(q, yes, no);
  area.appendChild(c);
}

function removeTile(id, announce) {
  const key = boardOf(id);
  const t = findTile(id);
  boards[key].splice(boards[key].indexOf(t), 1);
  stopPlay();
  closeIPick(false);
  draft = null;
  selectedId = null;
  render();
  renderEditor();
  if (id === "live") renderMacro(null);
  if (announce) say(`Deleted ${t.label}`);
}

function refreshPreview() {
  if (!draft) return;
  fillTile(edPreview, draft, stSel);
  const n = draft.states.length;
  $("ed-preview-note").textContent = n > 1 ? `Showing state ${stSel + 1} of ${n}: ${draft.states[stSel].name}` : "";
  // Square cells, like the board and the button designer: 1 × 1 is a square, 2 × 1 two squares
  const U = 80, G = 10;
  edPreview.style.flex = "none";
  edPreview.style.maxWidth = "none";
  edPreview.style.width = `${U * draft.w + G * (draft.w - 1)}px`;
  edPreview.style.height = `${U * draft.h + G * (draft.h - 1)}px`;
  let strip = $("ed-preview-states");
  if (!strip) {
    strip = Object.assign(document.createElement("div"), { className: "pstates", id: "ed-preview-states" });
    strip.setAttribute("role", "group");
    strip.setAttribute("aria-label", "All states");
    $("ed-preview-bg").appendChild(strip);
  }
  strip.innerHTML = "";
  strip.hidden = n < 2;
  if (n < 2) return;
  draft.states.forEach((st, i) => {
    const b = Object.assign(document.createElement("button"), { type: "button", className: "pstate" });
    b.setAttribute("aria-pressed", String(i === stSel));
    b.setAttribute("aria-label", `State ${i + 1}: ${st.name}${i === stSel ? ", shown above" : ""}`);
    const face = Object.assign(document.createElement("div"), { className: "tile preview mini" });
    face.setAttribute("aria-hidden", "true");
    fillTile(face, draft, i);
    const cap = Object.assign(document.createElement("small"), { textContent: `${i + 1}. ${st.name}` });
    b.append(face, cap);
    b.addEventListener("click", () => { if (i !== stSel) selectState(i); });
    strip.appendChild(b);
  });
}

let lastHelperSay = "", helperHopTimer = null, helperReady = false;
setTimeout(() => { helperReady = true; }, 600);
function refreshStatus() {
  const mate = $("ed-mate"), emote = $("ed-emote"), sayEl = $("ed-say");
  if (!draft) {
    mate.innerHTML = minnion({ c: pal("sky"), expr: "sleepy", scale: 3 });
    emote.textContent = "z";
    sayEl.textContent = "Nothing to edit yet.";
    return;
  }
  const probs = validate(draft);
  const warns = pluginWarnings(draft);
  // Inline errors: only after a field was left, or after a save attempt
  const nameErr = probs.find((p) => p.where === "name");
  const showName = showAll || touched.has("name");
  $("err-name").textContent = showName && nameErr ? nameErr.msg : "";
  edName.setAttribute("aria-invalid", String(!!(showName && nameErr)));
  document.querySelectorAll("#ed-fields [data-where]").forEach((ctrl) => {
    const p = probs.find((x) => x.where === ctrl.dataset.where);
    const show = !!p && (showAll || touched.has(ctrl.dataset.where));
    $(`err-${ctrl.id}`).textContent = show ? p.short : "";
    ctrl.setAttribute("aria-invalid", String(show));
  });
  // The helper minnion
  if (probs.length) {
    mate.innerHTML = minnion({ c: pal("sky"), expr: "surprised", scale: 3 });
    emote.textContent = "?";
    sayEl.textContent = probs[0].msg;
  } else if (warns.length) {
    const w = warns[0];
    mate.innerHTML = minnion({ c: pal("coral", "var(--sun)"), expr: "dizzy", scale: 3 });
    emote.textContent = "!";
    sayEl.textContent = `${w.plugin} is ${STATE_WORDS[w.state]}. You can save, but it will not run until ${w.plugin} is ready.`;
  } else {
    mate.innerHTML = minnion({ c: pal("mint"), expr: "happy", scale: 3 });
    emote.textContent = "✓";
    sayEl.textContent = isNew ? "Ready to add." : "Ready to save.";
  }
  // The helper hops and its bubble pops whenever its message changes
  if (sayEl.textContent !== lastHelperSay) {
    lastHelperSay = sayEl.textContent;
    if (!reduced && helperReady) {
      const h = mate.closest(".helper");
      emote.classList.remove("pop"); void emote.offsetWidth; emote.classList.add("pop");
      h.classList.remove("hop"); void h.offsetWidth; h.classList.add("hop");
      clearTimeout(helperHopTimer);
      helperHopTimer = setTimeout(() => h.classList.remove("hop"), 320);
    }
  }
  $("ed-problems").textContent = probs.length ? `${probs.length} ${probs.length === 1 ? "thing" : "things"} to fix` : "";
}

function refreshAll() {
  refreshPreview();
  refreshStatus();
  if (draft && draft.mode === "sequence") {
    // Keep the step list summaries in step with the fields without re-rendering the fields
    const picks = document.querySelectorAll("#ed-seq .seq-pick");
    const probs = validate(draft);
    picks.forEach((pick, i) => {
      if (!draft.steps[i]) return;
      pick.querySelector("small").textContent = summary(draft.steps[i]);
      pick.classList.toggle("bad", probs.some((p) => p.step === i) && (showAll || [...touched].some((k) => k.startsWith(`${i}:`))));
    });
  }
}

edName.addEventListener("input", () => { draft.label = edName.value; refreshAll(); });
edName.addEventListener("blur", () => { touched.add("name"); refreshAll(); });
$("ed-cancel").addEventListener("click", () => {
  if (isNew) { const label = "New button"; removeTile(selectedId, false); say(`${label} discarded`); return; }
  ownBefore.forEach((d) => { const i = DESIGNS.findIndex((x) => x.id === d.id); if (i >= 0) DESIGNS[i] = d; });
  openEditor(selectedId, false);
  say("Changes undone");
});
edForm.addEventListener("submit", (e) => {
  e.preventDefault();
  stopPlay();
  const probs = validate(draft);
  if (probs.length) {
    showAll = true;
    const first = probs[0];
    if (first.step >= 0 && first.step !== sel) { sel = first.step; renderSequence(); renderActionArea(); }
    refreshAll();
    if (!reduced) document.querySelectorAll('#ed-form [aria-invalid="true"]').forEach((el) => { el.classList.remove("err-shake"); void el.offsetWidth; el.classList.add("err-shake"); });
    const target = first.where === "name" ? edName : document.querySelector(`#ed-fields [data-where="${first.where}"]`);
    if (target) target.focus();
    return;
  }
  const t = findTile(selectedId);
  draft.states.forEach((st, i) => { st.name = st.name.trim() || `State ${i + 1}`; });
  const keepLoop = !!t._looping && draft.anim.repeat === "loop";
  // A knob button turns (the mouse wheel here, a drag on a phone)
  const act = ACTION[draft.steps[0] && draft.steps[0].type];
  draft.knob = draft.mode === "single" && !!(act && act.knob);
  Object.assign(t, clone({ ...draft, label: draft.label.trim(), cur: Math.min(t.cur || 0, draft.states.length - 1), _looping: keepLoop }));
  hideEditor();
  const wasNew = isNew;
  isNew = false;
  render();
  renderEditor();
  if (t.id === "live") renderMacro(t);
  const el = grid.querySelector(`[data-id="${t.id}"]`);
  const verb = wasNew ? "added" : "saved";
  if (el) dispatch(el, `${verb} ${t.label}`); else say(`${t.label} ${verb}`);
});

