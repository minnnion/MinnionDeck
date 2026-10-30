function renderMode() {
  segment($("ed-mode"), [["single", "One action"], ["sequence", "A sequence"]], draft.mode, (v) => {
    draft.mode = v;
    sel = 0;
    renderMode(); renderSequence(); renderActionArea(); renderLook(); refreshAll();
  }, (v) => v === "single" && draft.steps.length > 1);
  $("ed-mode-help").textContent = draft.mode === "sequence" && draft.steps.length > 1
    ? "Remove steps until one is left to switch back to one action."
    : draft.mode === "sequence" ? "Steps run in order, top to bottom." : "";
}

function renderSequence() {
  const box = $("ed-seq");
  box.hidden = draft.mode !== "sequence";
  if (box.hidden) return;
  box.innerHTML = "";
  const cap = document.createElement("span");
  cap.className = "cap";
  cap.textContent = "Steps";
  const list = document.createElement("div");
  list.className = "seq-list";
  const probs = validate(draft);
  draft.steps.forEach((s, i) => {
    const row = document.createElement("div");
    row.className = "seq-row";
    if (!reduced && i === freshStep) row.classList.add("fresh");
    if (!reduced && i === movedStep) row.classList.add("moved");
    const pick = document.createElement("button");
    pick.type = "button";
    pick.className = "seq-pick";
    if (probs.some((p) => p.step === i) && (showAll || [...touched].some((k) => k.startsWith(`${i}:`)))) pick.classList.add("bad");
    pick.setAttribute("aria-pressed", String(i === sel));
    pick.innerHTML = `${actionIcon(ACTION[s.type])}<span class="seq-txt"><span></span><small></small></span>`;
    pick.querySelector(".seq-txt span").textContent = `${i + 1}. ${ACTION[s.type].label}`;
    pick.querySelector("small").textContent = summary(s);
    pick.addEventListener("click", () => { sel = i; searchText = ""; renderSequence(); renderActionArea(); refreshAll(); });
    const mk = (label, aria, fn, off) => {
      const b = document.createElement("button");
      b.type = "button"; b.className = "btn sm"; b.textContent = label;
      b.setAttribute("aria-label", aria); b.disabled = off;
      b.addEventListener("click", fn);
      return b;
    };
    const swap = (a, b) => {
      [draft.steps[a], draft.steps[b]] = [draft.steps[b], draft.steps[a]];
      sel = b;
      movedStep = b;
      renderSequence(); renderActionArea(); refreshAll();
    };
    row.append(
      pick,
      mk("↑", `Move step ${i + 1} up`, () => swap(i, i - 1), i === 0),
      mk("↓", `Move step ${i + 1} down`, () => swap(i, i + 1), i === draft.steps.length - 1),
      mk("✕", `Remove step ${i + 1}`, () => {
        draft.steps.splice(i, 1);
        sel = Math.min(sel, draft.steps.length - 1);
        renderMode(); renderSequence(); renderActionArea(); refreshAll();
      }, draft.steps.length === 1)
    );
    list.appendChild(row);
  });
  freshStep = -1;
  movedStep = -1;
  const add = document.createElement("button");
  add.type = "button";
  add.className = "btn";
  add.textContent = "Add step";
  add.addEventListener("click", () => {
    draft.steps.push(newStep("key"));
    sel = draft.steps.length - 1;
    freshStep = sel;
    renderMode(); renderSequence(); renderActionArea(); refreshAll();
    openPick();
  });
  box.append(cap, list, add);
}

function renderActionArea() {
  const box = $("ed-action");
  box.innerHTML = "";
  pickOpen = false;
  const head = document.createElement("span");
  head.className = "cap";
  head.id = "cap-action";
  head.textContent = draft.mode === "sequence" ? `Step ${sel + 1}: action` : "Action";

  const wrap = document.createElement("div");
  wrap.className = "pick";
  wrap.id = "ed-pick";
  const btn = document.createElement("button");
  btn.type = "button";
  btn.className = "pick-btn";
  btn.id = "ed-pick-btn";
  btn.setAttribute("aria-haspopup", "listbox");
  btn.setAttribute("aria-expanded", "false");
  btn.setAttribute("aria-labelledby", "cap-action ed-pick-btn");
  const pop = document.createElement("div");
  pop.className = "pick-pop";
  pop.id = "ed-pick-pop";
  pop.hidden = true;
  const search = document.createElement("input");
  search.id = "ed-search";
  search.type = "search";
  search.placeholder = "Search actions";
  search.autocomplete = "off";
  search.setAttribute("role", "combobox");
  search.setAttribute("aria-label", "Search actions");
  search.setAttribute("aria-autocomplete", "list");
  search.setAttribute("aria-controls", "ed-picker");
  search.setAttribute("aria-expanded", "false");
  const list = document.createElement("div");
  list.className = "picker";
  list.id = "ed-picker";
  list.setAttribute("role", "listbox");
  list.setAttribute("aria-labelledby", "cap-action");
  pop.append(search, list);
  wrap.append(btn, pop);

  btn.addEventListener("click", () => (pickOpen ? closePick(false) : openPick()));
  btn.addEventListener("keydown", (e) => { if (e.key === "ArrowDown" || e.key === "ArrowUp") { e.preventDefault(); openPick(); } });
  search.addEventListener("input", () => { searchText = search.value; pickActiveId = null; buildPickList(); });
  search.addEventListener("keydown", (e) => {
    const i = pickOpts.findIndex((o) => o.dataset.id === pickActiveId);
    if (e.key === "ArrowDown") setPickActive(i + 1);
    else if (e.key === "ArrowUp") setPickActive(i - 1);
    else if (e.key === "PageDown") setPickActive(i + 5);
    else if (e.key === "PageUp") setPickActive(i - 5);
    else if (e.key === "Enter") { if (pickActiveId) chooseAction(pickActiveId); }
    else if (e.key === "Escape") { e.stopPropagation(); closePick(true); }
    else if (e.key === "Tab") { closePick(false); return; }
    else return;
    e.preventDefault();
  });

  const note = document.createElement("span");
  note.className = "icon-note";
  note.id = "ed-icon-note";
  const fields = document.createElement("div");
  fields.id = "ed-fields";
  fields.className = "stack";
  const warn = document.createElement("div");
  warn.id = "ed-warn";
  box.append(head, wrap, note, warn, fields);
  renderPicker();
  renderFields();
  renderWarn();
}

