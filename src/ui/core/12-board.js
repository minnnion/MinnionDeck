// ===== Board =====
function render() {
  const tiles = boards[current];
  grid.innerHTML = "";
  grid.hidden = tiles.length === 0;
  emptyEl.hidden = tiles.length !== 0;
  tiles.forEach((t) => {
    const b = document.createElement("button");
    b.className = "tile";
    b.dataset.id = t.id;
    b.style.gridColumn = `span ${t.w}`;
    b.style.gridRow = `span ${t.h}`;
    fillTile(b, t);
    resumeAnim(b, t);
    const handle = document.createElement("span");
    handle.className = "handle";
    handle.dataset.resize = "1";
    handle.setAttribute("aria-hidden", "true");
    b.appendChild(handle);
    b.setAttribute("aria-label", tileLabel(t));
    b.classList.toggle("selected", t.id === selectedId);
    grid.appendChild(b);
  });
  grid.classList.toggle("edit", editMode);
}

// A minnion squashes, runs to the tile, lands, hops, and walks back.
async function dispatch(tileEl, message) {
  if (busy || reduced) { say(message); return; }
  busy = true;
  const idx = nextRunner;
  nextRunner = (nextRunner + 1) % crew.length;
  const m = crew[idx];
  const seatSprite = bench.querySelector(`[data-i="${idx}"] svg`);
  const br = boardEl.getBoundingClientRect();
  const sr = seatSprite.getBoundingClientRect();
  const tr = tileEl.getBoundingClientRect();
  const from = { x: sr.left - br.left, y: sr.top - br.top };
  const to = { x: tr.left - br.left + tr.width / 2 - 24, y: tr.top - br.top - sr.height + 8 };
  const goLook = to.x < from.x ? -1 : 1;

  const runner = document.createElement("div");
  runner.className = "runner";
  runner.style.transform = `translate(${from.x}px, ${from.y}px)`;
  const body = document.createElement("div");
  const emote = document.createElement("span");
  emote.className = "emote";
  emote.textContent = "!";
  runner.append(body, emote);
  boardEl.appendChild(runner);
  renderSeat(idx, true);
  mates(idx, { expr: "surprised", look: goLook });

  let step = 0;
  let lookDir = goLook;
  const draw = (expr = "open", feet, pose = "") => {
    body.innerHTML = minnion({ c: pal(m.body), expr, look: lookDir, tall: m.tall, feet: feet || (step++ % 2 ? "a" : "b"), pose, scale: 3 });
  };
  const move = (a, b) => runner.animate(
    [{ transform: `translate(${a.x}px, ${a.y}px)` }, { transform: `translate(${b.x}px, ${b.y}px)` }],
    { duration: 480, easing: "steps(8, end)", fill: "forwards" }
  ).finished;
  let walker;
  try {
    draw("surprised", "idle", "squash");
    await wait(100);
    draw();
    walker = setInterval(() => draw(), 90);
    await move(from, to);
    clearInterval(walker);
    draw("open", "idle", "squash");
    await wait(80);
    emote.textContent = "✓";
    draw("happy", "idle", "stretch");
    await runner.animate(
      [{ transform: `translate(${to.x}px, ${to.y}px)` }, { transform: `translate(${to.x}px, ${to.y - 8}px)` }, { transform: `translate(${to.x}px, ${to.y}px)` }],
      { duration: 160, easing: "steps(2, end)", fill: "forwards" }
    ).finished;
    draw("happy", "idle");
    say(`${m.name} ${message}`);
    mates(idx, { expr: "happy", look: 0 }, true);
    emote.remove();
    lookDir = -goLook;
    walker = setInterval(() => draw(), 90);
    await move(to, from);
  } finally {
    clearInterval(walker);
    runner.remove();
    crew.forEach((c) => { c.expr = "open"; c.look = 0; });
    renderBench();
    busy = false;
  }
}

grid.addEventListener("click", (e) => {
  const tileEl = e.target.closest(".tile");
  if (!tileEl) return;
  const t = findTile(tileEl.dataset.id);
  if (editMode) {
    if (e.target.closest("[data-resize]")) {
      const k = SIZES.findIndex(([w, h]) => w === t.w && h === t.h);
      [t.w, t.h] = SIZES[(k + 1) % SIZES.length];
      render();
      const fresh = grid.querySelector(`[data-id="${t.id}"]`);
      fresh.classList.add("bump");
      if (t.id === selectedId && draft) { draft.w = t.w; draft.h = t.h; refreshPreview(); }
      dispatch(fresh, `resized ${t.label} to ${t.w} x ${t.h}`);
    } else {
      openEditor(t.id, false);
      say(`Editing ${t.label}`);
      showEditor();
    }
    return;
  }
  if (t.mode === "sequence") {
    pressAnim(tileEl, t);
    advanceTile(t, tileEl);
    dispatch(tileEl, `started ${t.label}. Progress is in Macro run.`);
    runMacro(t);
    return;
  }
  const owner = tileOwner(t);
  if (owner && pluginState(owner) !== "ready") {
    tileEl.classList.remove("denied"); void tileEl.offsetWidth; tileEl.classList.add("denied");
    say(`Could not run ${t.label}. ${owner} is ${STATE_WORDS[pluginState(owner)]}.`);
    return;
  }
  pressAnim(tileEl, t);
  advanceTile(t, tileEl);
  dispatch(tileEl, `ran ${t.label}`);
});

// Move a pressed button to its next state
function advanceTile(t, tileEl) {
  if (t.states.length > 1) {
    t.cur = ((t.cur || 0) + 1) % t.states.length;
    fillTile(tileEl, t);
    const h = document.createElement("span"); h.className = "handle"; h.dataset.resize = "1"; h.setAttribute("aria-hidden", "true"); tileEl.appendChild(h);
    tileEl.setAttribute("aria-label", tileLabel(t));
  }
  // Keep the same button in step everywhere it shows: board, app window and phones
  if (window.syncTileEls) window.syncTileEls(t, tileEl);
}

$("edit-toggle").addEventListener("click", (e) => {
  editMode = !editMode;
  e.currentTarget.setAttribute("aria-pressed", String(editMode));
  e.currentTarget.textContent = editMode ? "Done editing" : "Edit layout";
  render();
  say(editMode ? "Edit mode on. Tap a tile to edit it, or its corner to resize." : "Layout saved");
});

document.querySelectorAll("[data-board]").forEach((tab) => {
  tab.addEventListener("click", () => {
    document.querySelectorAll("[data-board]").forEach((x) => x.setAttribute("aria-selected", String(x === tab)));
    current = tab.dataset.board;
    render();
    say(current === "Empty" ? "New board created" : `Board: ${current}`);
  });
});

$("add-first").addEventListener("click", () => {
  const t = T(`new${Date.now()}`, "", "bolt", 1, 1, "single", [newStep("key")]);
  boards.Empty.push(t);
  render();
  openEditor(t.id, true);
  say("New button added. Set it up in the editor.");
  showEditor();
});

