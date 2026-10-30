// ----- States strip and the card for the selected state -----
function renderStates() {
  const strip = $("ed-state-strip");
  strip.innerHTML = "";
  const n = draft.states.length;
  draft.states.forEach((st, i) => {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "state-chip";
    b.dataset.i = i;
    b.setAttribute("aria-pressed", String(i === stSel));
    b.setAttribute("aria-label", `State ${i + 1} of ${n}: ${st.name}`);
    const sbg = stateBg(st);
    b.innerHTML = `<span class="sw" style="background:${sbg.css.replace(/"/g, "'")};color:${textOn(st.bg)}">${sbg.design ? "" : glyph(st.glyph, 16)}</span><span><span class="n"></span><br><span class="nm"></span></span>`;
    if (sbg.design) { const sw = b.querySelector(".sw"); sw.style.position = "relative"; sw.style.overflow = "hidden"; sw.innerHTML = designHTML(sbg.design, draft.w, draft.h); }
    b.querySelector(".n").textContent = n > 1 ? `${i + 1}` : "Look";
    b.querySelector(".nm").textContent = st.name;
    if (i === justAdded && !reduced) b.classList.add("chip-in");
    b.addEventListener("click", () => { stopPlay(); selectState(i); });
    strip.appendChild(b);
  });
  justAdded = -1;
  const add = document.createElement("button");
  add.type = "button";
  add.className = "state-chip add";
  add.textContent = "+ Add state";
  add.disabled = n >= MAX_STATES;
  add.addEventListener("click", addState);
  strip.appendChild(add);
  const names = draft.states.map((st) => st.name);
  $("ed-states-help").textContent = n === 1
    ? "One look. Add a state to make the button change each time you press it."
    : n >= MAX_STATES
      ? `Each press moves to the next state: ${names.join(" → ")}, then back to ${names[0]}. Six states is the most a button can have.`
      : `Each press moves to the next state: ${names.join(" → ")}, then back to ${names[0]}.`;
  $("ed-play").disabled = n < 2;
  $("sum-states").textContent = n === 1 ? "One look" : `${n}: ${names.join(" → ")}`;
  if ($("sum-gif")) gifSummary();
}
function renderStateCard() {
  const n = draft.states.length;
  const st = draft.states[stSel];
  $("ed-state-title").textContent = n > 1 ? `State ${stSel + 1} of ${n}: ${st.name}` : "Look of this button";
  $("ed-state-name-field").hidden = n === 1;
  $("ed-state-name").value = st.name;
  const text = $("ed-state-text");
  text.value = st.text;
  text.placeholder = draft.label.trim() || "Same as the button name";
  const tools = $("ed-state-tools");
  tools.innerHTML = "";
  if (n > 1) {
    const mk = (label, aria, fn, off) => {
      const b = Object.assign(document.createElement("button"), { type: "button", className: "btn sm", textContent: label });
      b.setAttribute("aria-label", aria); b.disabled = off; b.addEventListener("click", fn);
      return b;
    };
    tools.append(
      mk("←", "Move this state earlier", () => moveState(-1), stSel === 0),
      mk("→", "Move this state later", () => moveState(1), stSel === n - 1),
      mk("Remove", `Remove state ${st.name}`, removeState, false)
    );
  }
  renderIconPicker();
  renderBg(st);
}
function selectState(i) {
  stSel = i;
  if (draft) { renderAnimPicker(); renderGif(); }
  closeIPick(false);
  renderStates();
  renderStateCard();
  flipPreview();
  refreshAll();
}
function addState() {
  if (draft.states.length >= MAX_STATES) return;
  const from = draft.states[stSel];
  if (draft.states.length === 1 && draft.states[0].name === "Default") draft.states[0].name = "State 1";
  const used = new Set(draft.states.map((st) => st.bg));
  const bg = (COLORS.find(([tok]) => !used.has(tok)) || COLORS[0])[0];
  draft.states.push({ ...clone(from), name: `State ${draft.states.length + 1}`, text: "", bg, bgType: from.bgType === "colour" || !from.bgType ? "colour" : from.bgType });
  justAdded = draft.states.length - 1;
  stopPlay();
  selectState(justAdded);
  $("ed-state-name").focus();
  $("ed-state-name").select();
}
async function removeState() {
  if (draft.states.length < 2) return;
  const chip = $("ed-state-strip").querySelector(`[data-i="${stSel}"]`);
  stopPlay();
  if (chip && !reduced) await chip.animate([{ transform: "scale(1)" }, { transform: "scale(0.3)", opacity: 0 }], { duration: 160, easing: "steps(3, end)", fill: "forwards" }).finished;
  draft.states.splice(stSel, 1);
  selectState(Math.min(stSel, draft.states.length - 1));
  $("ed-state-strip").querySelector('[aria-pressed="true"]').focus();
}
function moveState(dir) {
  const j = stSel + dir;
  [draft.states[stSel], draft.states[j]] = [draft.states[j], draft.states[stSel]];
  justAdded = j;
  selectState(j);
}
$("ed-state-name").addEventListener("input", (e) => {
  draft.states[stSel].name = e.target.value;
  renderStates();
  $("ed-state-title").textContent = `State ${stSel + 1} of ${draft.states.length}: ${e.target.value}`;
  refreshAll();
});
$("ed-state-name").addEventListener("blur", (e) => {
  if (!draft || !draft.states[stSel] || e.target.value.trim()) return;
  draft.states[stSel].name = `State ${stSel + 1}`;
  renderStates(); renderStateCard(); refreshAll();
});
$("ed-state-text").addEventListener("input", (e) => { draft.states[stSel].text = e.target.value; refreshAll(); });

