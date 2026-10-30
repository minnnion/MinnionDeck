// ----- Press animation: dropdown of ready-made animations, speed, repeat, phones -----
let apickOpen = false, apickActive = null, apickOpts = [];
function renderAnim() {
  if (!draft.anim) draft.anim = defaultAnim();
  const a = draft.anim;
  const none = a.type === "none";
  renderAnimPicker();
  const seg = (id, opts, val, fn) => segment($(id), opts, val, (v) => { fn(v); renderAnim(); const b = $(id).querySelector('[aria-pressed="true"]'); if (b) b.focus(); }, () => none);
  seg("ed-anim-speed", [["slow", "Slow"], ["normal", "Normal"], ["fast", "Fast"]], a.speed, (v) => { a.speed = v; previewAnim(); });
  seg("ed-anim-rep", [["once", "Once"], ["three", "3 times"], ["loop", "Until pressed again"]], a.repeat, (v) => { a.repeat = v; previewAnim(); });
  seg("ed-anim-phone", [[true, "Play on phones too"], [false, "This PC only"]], a.phone !== false, (v) => { a.phone = v; });
  $("ed-anim-help").textContent = none ? "This button does not move when pressed."
    : a.repeat === "loop" ? "Keeps playing until the button is pressed again. Good for things that stay on, like going live. The preview shows it three times."
    : a.repeat === "three" ? "Plays three times on each press." : "Plays once on each press.";
  $("ed-anim-play").disabled = none;
  $("sum-anim").textContent = none ? "None" : `${(ANIM[a.type] || ANIM.press).name}, ${a.repeat === "loop" ? "until pressed again" : a.repeat === "three" ? "3 times" : "once"}`;
}
function renderAnimPicker() {
  const area = $("ed-anim-area");
  area.innerHTML = "";
  apickOpen = false;
  const a = draft.anim, cur = ANIM[a.type] || ANIM.press;
  const wrap = Object.assign(document.createElement("div"), { className: "pick", id: "ed-apick" });
  const btn = Object.assign(document.createElement("button"), { type: "button", className: "pick-btn", id: "ed-apick-btn" });
  btn.setAttribute("aria-haspopup", "listbox");
  btn.setAttribute("aria-expanded", "false");
  btn.setAttribute("aria-labelledby", "cap-anim ed-apick-btn");
  btn.innerHTML = `<span class="amini" aria-hidden="true">${glyph(draft.states[stSel].glyph, 14)}</span><span class="pick-txt"><span></span><small></small></span>${glyph("caret", 16)}`;
  btn.querySelector(".pick-txt span").textContent = cur.name;
  btn.querySelector(".pick-txt small").textContent = cur.desc;
  const pop = Object.assign(document.createElement("div"), { className: "pick-pop", id: "ed-apick-pop" });
  pop.hidden = true;
  const list = Object.assign(document.createElement("div"), { className: "picker", id: "ed-anims", tabIndex: -1 });
  list.setAttribute("role", "listbox");
  list.setAttribute("aria-labelledby", "cap-anim");
  pop.appendChild(list);
  wrap.append(btn, pop);
  area.appendChild(wrap);
  btn.addEventListener("click", () => (apickOpen ? closeAPick(true) : openAPick()));
  btn.addEventListener("keydown", (e) => { if (e.key === "ArrowDown" || e.key === "ArrowUp") { e.preventDefault(); openAPick(); } });
  list.addEventListener("keydown", (e) => {
    const i = apickOpts.findIndex((o) => o.dataset.id === apickActive);
    if (e.key === "ArrowDown") setAActive(i + 1);
    else if (e.key === "ArrowUp") setAActive(i - 1);
    else if (e.key === "Home") setAActive(0);
    else if (e.key === "End") setAActive(apickOpts.length - 1);
    else if (e.key === "Enter" || e.key === " ") chooseAnim(apickActive);
    else if (e.key === "Escape") { e.stopPropagation(); closeAPick(true); }
    else if (e.key === "Tab") { closeAPick(false); return; }
    else return;
    e.preventDefault();
  });
}
function buildAnimList() {
  const list = $("ed-anims");
  list.innerHTML = "";
  apickOpts = [];
  const g8 = draft.states[stSel].glyph;
  ANIM_GROUPS.forEach((gname, gi) => {
    const group = document.createElement("div");
    group.setAttribute("role", "group");
    group.setAttribute("aria-labelledby", `agrp-${gi}`);
    const head = Object.assign(document.createElement("div"), { className: "grp", id: `agrp-${gi}`, textContent: gname });
    group.appendChild(head);
    ANIMS.filter((x) => x.group === gname).forEach((x) => {
      const o = Object.assign(document.createElement("div"), { className: "opt", id: `an-${x.id}` });
      o.dataset.id = x.id;
      o.setAttribute("role", "option");
      o.setAttribute("aria-selected", String(x.id === draft.anim.type));
      o.innerHTML = `<span class="amini" aria-hidden="true">${glyph(g8, 14)}</span><span class="opt-txt"><span></span><small></small></span>${x.id === draft.anim.type ? '<span class="opt-check" aria-hidden="true">✓</span>' : ""}`;
      o.querySelector(".opt-txt span").textContent = x.name;
      o.querySelector(".opt-txt small").textContent = x.desc;
      o.addEventListener("click", () => chooseAnim(x.id));
      o.addEventListener("mousemove", () => { if (apickActive !== x.id) setAActive(apickOpts.indexOf(o), false); });
      group.appendChild(o);
      apickOpts.push(o);
    });
    list.appendChild(group);
  });
}
// The highlighted option plays its animation on its little sample tile
function setAActive(i, scroll = true) {
  const k = Math.max(0, Math.min(apickOpts.length - 1, i));
  apickActive = apickOpts[k].dataset.id;
  apickOpts.forEach((o, j) => {
    o.classList.toggle("active", j === k);
    const m = o.querySelector(".amini");
    if (j === k) playAnim(m, { type: o.dataset.id, speed: draft.anim.speed }, true); else stopAnim(m);
  });
  $("ed-anims").setAttribute("aria-activedescendant", apickOpts[k].id);
  if (!scroll) return;
  const list = $("ed-anims"), o = apickOpts[k];
  const lr = list.getBoundingClientRect(), orr = o.getBoundingClientRect(), head = 30;
  if (orr.top < lr.top + head) list.scrollTop -= lr.top + head - orr.top;
  else if (orr.bottom > lr.bottom) list.scrollTop += orr.bottom - lr.bottom;
}
function openAPick() {
  if (apickOpen) return;
  closePick(false);
  closeIPick(false);
  closeGPick(false);
  apickOpen = true;
  showPop($("ed-apick-pop"));
  $("ed-apick-btn").setAttribute("aria-expanded", "true");
  buildAnimList();
  setAActive(Math.max(0, apickOpts.findIndex((o) => o.dataset.id === draft.anim.type)));
  $("ed-apick-pop").scrollIntoView({ block: "nearest" });
  $("ed-anims").focus({ preventScroll: true });
}
function closeAPick(refocus) {
  if (!apickOpen) return;
  apickOpen = false;
  apickOpts.forEach((o) => stopAnim(o.querySelector(".amini")));
  hidePop($("ed-apick-pop"));
  $("ed-apick-btn").setAttribute("aria-expanded", "false");
  if (refocus) $("ed-apick-btn").focus();
}
function chooseAnim(id) {
  if (!id) return;
  draft.anim.type = id;
  closeAPick(false);
  renderAnim();
  $("ed-apick-btn").focus();
  previewAnim();
  refreshAll();
}
document.addEventListener("pointerdown", (e) => { if (apickOpen && !e.target.closest("#ed-apick")) closeAPick(false); });
$("ed-anim-play").addEventListener("click", previewAnim);

