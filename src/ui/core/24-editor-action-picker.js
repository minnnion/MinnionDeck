// Dropdown state. The list is built only while it is open, so plugin status is always current.
let pickOpen = false, pickActiveId = null, pickOpts = [];
function renderPicker() {
  const btn = $("ed-pick-btn");
  if (!btn) return;
  const a = ACTION[draft.steps[sel].type];
  btn.innerHTML = `${actionIcon(a)}<span class="pick-txt"><span></span><small></small></span>${glyph("caret", 16)}`;
  btn.querySelector(".pick-txt span").textContent = a.label;
  btn.querySelector(".pick-txt small").textContent = a.plugin ? `${a.plugin} plugin` : a.group;
  btn.title = `Icon: ${iconPath(a)}`;
  $("ed-icon-note").textContent = `Icon from ${iconPath(a)}`;
  if (pickOpen) buildPickList();
}
function buildPickList() {
  const list = $("ed-picker");
  const step = draft.steps[sel];
  const q = searchText.trim().toLowerCase();
  list.innerHTML = "";
  pickOpts = [];
  const installed = (a) => !a.pluginId || a.id === step.type || plugins.some((p) => p.id === a.pluginId);
  GROUPS.forEach((g, gi) => {
    const items = ACTIONS.filter((a) => a.group === g && installed(a) && (!q || a.label.toLowerCase().includes(q) || g.toLowerCase().includes(q)));
    if (!items.length) return;
    const plugin = items[0].plugin;
    const st = plugin ? pluginState(plugin) : null;
    const group = document.createElement("div");
    group.setAttribute("role", "group");
    group.setAttribute("aria-labelledby", `grp-${gi}`);
    const head = document.createElement("div");
    head.className = "grp";
    head.id = `grp-${gi}`;
    head.innerHTML = `${plugin ? minnion({ c: st === "dizzy" ? pal("coral", "var(--sun)") : pal(PLUGIN_BODY[plugin]), expr: st === "ready" ? "open" : st === "dizzy" ? "dizzy" : "sleepy", scale: 1 }) : ""}<span></span><small></small><span class="grp-path" aria-hidden="true"></span>`;
    head.querySelector("span").textContent = g;
    head.querySelector("small").textContent = plugin ? (st === "ready" ? "Ready" : st === "missing" ? "Not installed" : st === "dizzy" ? "Stopped" : st === "off" ? "Turned off" : "Starting up") : "Built in";
    head.querySelector(".grp-path").textContent = pluginFolder(items[0]);
    group.appendChild(head);
    items.forEach((a) => {
      const o = document.createElement("div");
      o.className = "opt";
      o.id = `opt-${a.id}`;
      o.dataset.id = a.id;
      o.setAttribute("role", "option");
      o.setAttribute("aria-selected", String(a.id === step.type));
      if (st === "missing") o.setAttribute("aria-disabled", "true");
      o.title = iconPath(a);
      o.innerHTML = `${actionIcon(a)}<span class="opt-txt"></span>${a.id === step.type ? '<span class="opt-check" aria-hidden="true">✓</span>' : ""}`;
      o.querySelector(".opt-txt").textContent = a.label;
      o.addEventListener("pointerdown", (e) => e.preventDefault()); // keep focus in the search box
      o.addEventListener("click", () => chooseAction(a.id));
      o.addEventListener("mousemove", () => { if (pickActiveId !== a.id) setPickActive(pickOpts.indexOf(o), false); });
      group.appendChild(o);
      pickOpts.push(o);
    });
    list.appendChild(group);
  });
  if (!pickOpts.length) {
    const none = document.createElement("div");
    none.className = "none";
    none.textContent = `No action matches "${searchText}". Try a shorter word.`;
    list.appendChild(none);
    pickActiveId = null;
    $("ed-search").removeAttribute("aria-activedescendant");
    return;
  }
  const keep = pickOpts.findIndex((o) => o.dataset.id === pickActiveId);
  const cur = pickOpts.findIndex((o) => o.dataset.id === step.type);
  setPickActive(keep >= 0 ? keep : q ? 0 : Math.max(0, cur));
}
function setPickActive(i, scroll = true) {
  if (!pickOpts.length) return;
  const k = Math.max(0, Math.min(pickOpts.length - 1, i));
  pickActiveId = pickOpts[k].dataset.id;
  pickOpts.forEach((o, j) => o.classList.toggle("active", j === k));
  $("ed-search").setAttribute("aria-activedescendant", pickOpts[k].id);
  if (!scroll) return;
  const list = $("ed-picker"), o = pickOpts[k];
  const top = o.offsetTop - list.offsetTop, head = 34;
  if (top - head < list.scrollTop) list.scrollTop = Math.max(0, top - head);
  else if (top + o.offsetHeight > list.scrollTop + list.clientHeight) list.scrollTop = top + o.offsetHeight - list.clientHeight;
}
function openPick() {
  if (pickOpen || !$("ed-pick-btn")) return;
  closeIPick(false);
  closeAPick(false);
  closeGPick(false);
  pickOpen = true;
  searchText = "";
  pickActiveId = null;
  const search = $("ed-search"), pop = $("ed-pick-pop");
  search.value = "";
  showPop(pop);
  $("ed-pick-btn").setAttribute("aria-expanded", "true");
  search.setAttribute("aria-expanded", "true");
  buildPickList();
  pop.scrollIntoView({ block: "nearest" });
  search.focus({ preventScroll: true });
}
function closePick(refocus) {
  if (!pickOpen) return;
  pickOpen = false;
  hidePop($("ed-pick-pop"));
  $("ed-pick-btn").setAttribute("aria-expanded", "false");
  const search = $("ed-search");
  search.setAttribute("aria-expanded", "false");
  search.removeAttribute("aria-activedescendant");
  if (refocus) $("ed-pick-btn").focus();
}
function chooseAction(id) {
  const a = ACTION[id];
  if (a.plugin && pluginState(a.plugin) === "missing") return;
  if (id !== draft.steps[sel].type) {
    draft.steps[sel] = newStep(id);
    [...touched].filter((k) => k.startsWith(`${sel}:`)).forEach((k) => touched.delete(k));
    // A two-state action on a one-look button gets a state for each side. A plugin action with
    // its own states always gets exactly those, since the plugin switches between them by number.
    if (a.dual && draft.mode === "single" && (draft.states.length === 1 || a.ownStates)) {
      const base = draft.states[0];
      const fills = ["mint", "coral", "sky", "lilac", "peach", "sun"].filter((c) => c !== base.bg);
      draft.states = a.dual.map((name, i) => (i === 0 ? { ...base, name } : { ...base, name, bg: fills[(i - 1) % fills.length] }));
      stSel = 1;
      justAdded = 1;
    }
  }
  closePick(true);
  renderPicker(); renderFields(); renderWarn(); renderSequence(); renderLook(); refreshAll();
}
document.addEventListener("pointerdown", (e) => { if (pickOpen && !e.target.closest("#ed-pick")) closePick(false); });

