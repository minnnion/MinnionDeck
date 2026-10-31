  // ===== Deck: page grid =====
  function renderCrumb() {
    const hit = findNode(A.page);
    const parts = [prof().name, ...(hit ? hit.path.map((f) => f.name) : []), hit ? hit.node.name : ""];
    $("a-crumb").textContent = A.view === "deck" ? `— ${parts.join(" / ")}` : `— ${VIEWS.find(([id]) => id === A.view)[1]}`;
  }
  // ----- Every button has a cell on its page. Buttons without one are packed into the first gap. -----
  let CELL_H = 84;
  const GAP = 6, MAX_H = 3;
  const clamp = (v, lo, hi) => Math.max(lo, Math.min(hi, v));
  function layoutPage(pg) {
    const cols = pg.cols, tiles = tilesOf(pg), own = new Map();
    const free = (x, y, w, h, skip) => {
      if (x < 0 || y < 0 || x + w > cols) return false;
      for (let yy = y; yy < y + h; yy++) for (let xx = x; xx < x + w; xx++) { const o = own.get(`${xx},${yy}`); if (o && o !== skip) return false; }
      return true;
    };
    const mark = (t) => { for (let yy = t.y; yy < t.y + t.h; yy++) for (let xx = t.x; xx < t.x + t.w; xx++) own.set(`${xx},${yy}`, t); };
    const later = [];
    tiles.forEach((t) => { t.w = Math.min(t.w, cols); if (t.x != null && free(t.x, t.y, t.w, t.h)) mark(t); else later.push(t); });
    later.forEach((t) => {
      for (let y = 0; ; y++) {
        const x = [...Array(cols - t.w + 1).keys()].find((xx) => free(xx, y, t.w, t.h));
        if (x !== undefined) { t.x = x; t.y = y; mark(t); break; }
      }
    });
    const rows = Math.max(pg.rows, ...tiles.map((t) => t.y + t.h));
    return { rows, own, free };
  }
  // A button the same size sitting exactly on the target spot can swap places
  function swapPartner(L, t, x, y) {
    const o = L.own.get(`${x},${y}`);
    return o && o !== t && o.x === x && o.y === y && o.w === t.w && o.h === t.h ? o : null;
  }
  function applyTarget(t, tg) {
    if (tg.swap) { tg.swap.x = t.x; tg.swap.y = t.y; say(`Swapped ${t.label} and ${tg.swap.label}`); }
    else if (tg.w !== t.w || tg.h !== t.h) say(`${t.label} is now ${tg.w} × ${tg.h}`);
    else say(`Moved ${t.label} to column ${tg.x + 1}, row ${tg.y + 1}`);
    Object.assign(t, { x: tg.x, y: tg.y, w: tg.w, h: tg.h });
    renderPhones(true);
  }
  const focusTile = (id) => { const x = $("a-grid") && $("a-grid").querySelector(`[data-id="${id}"]`); if (x) x.focus({ preventScroll: true }); return x; };
  const flash = (id, cls) => { const x = focusTile(id); if (!x || reduced) return; x.classList.remove(cls); void x.offsetWidth; x.classList.add(cls); };
  function nudge(pg, t, dx, dy, resize) {
    const L = layoutPage(pg);
    const tg = resize
      ? { x: t.x, y: t.y, w: clamp(t.w + dx, 1, pg.cols - t.x), h: clamp(t.h + dy, 1, MAX_H) }
      : { x: clamp(t.x + dx, 0, pg.cols - t.w), y: clamp(t.y + dy, 0, L.rows - t.h), w: t.w, h: t.h };
    if (tg.x === t.x && tg.y === t.y && tg.w === t.w && tg.h === t.h) { say(resize ? "That is as far as it can go." : "That is the edge of the page."); flash(t.id, "nope"); return; }
    tg.swap = resize ? null : swapPartner(L, t, tg.x, tg.y);
    if (!L.free(tg.x, tg.y, tg.w, tg.h, t) && !tg.swap) { say(resize ? "No room for that size." : "No room there."); flash(t.id, "nope"); return; }
    A.tile = t.id;
    applyTarget(t, tg);
    renderMain(); renderInsp();
    flash(t.id, "bump");
  }
  let lastTap = { id: null, at: 0 };
  function enableDrag(g, pg, L) {
    g.addEventListener("pointerdown", (e) => {
      const tileEl = e.target.closest(".tile");
      if (!tileEl || e.button !== 0) return;
      const t = tilesOf(pg).find((x) => x.id === tileEl.dataset.id);
      const resizing = !!e.target.closest(".rz");
      e.preventDefault();
      tileEl.focus({ preventScroll: true });
      tileEl.setPointerCapture(e.pointerId);
      const gr = g.getBoundingClientRect();
      const cw = (gr.width - GAP * (pg.cols - 1)) / pg.cols;
      const cellAt = (px, py) => ({ x: Math.floor((px - gr.left + GAP / 2) / (cw + GAP)), y: Math.floor((py - gr.top + GAP / 2) / (CELL_H + GAP)) });
      const start = cellAt(e.clientX, e.clientY);
      const off = { x: start.x - t.x, y: start.y - t.y };
      let moved = false, target = null;
      const ghost = el("div", { className: "drop-ghost" });
      ghost.setAttribute("aria-hidden", "true");
      const onMove = (ev) => {
        const dx = ev.clientX - e.clientX, dy = ev.clientY - e.clientY;
        if (!moved && Math.hypot(dx, dy) < 5) return;
        if (!moved) { moved = true; tileEl.classList.add(resizing ? "resizing" : "dragging"); g.appendChild(ghost); }
        const c = cellAt(ev.clientX, ev.clientY);
        if (resizing) {
          const w = clamp(c.x - t.x + 1, 1, pg.cols - t.x), h = clamp(c.y - t.y + 1, 1, MAX_H);
          target = { x: t.x, y: t.y, w, h, ok: L.free(t.x, t.y, w, h, t) };
          ghost.textContent = target.ok ? `${w} × ${h}` : "No room";
        } else {
          tileEl.style.transform = `translate(${dx}px, ${dy}px)`;
          const x = clamp(c.x - off.x, 0, pg.cols - t.w), y = clamp(c.y - off.y, 0, L.rows - t.h);
          const swap = swapPartner(L, t, x, y);
          target = { x, y, w: t.w, h: t.h, swap, ok: L.free(x, y, t.w, t.h, t) || !!swap };
          ghost.textContent = !target.ok ? "No room" : swap ? `Swap with ${swap.label}` : "";
        }
        ghost.style.gridColumn = `${target.x + 1} / span ${target.w}`;
        ghost.style.gridRow = `${target.y + 1} / span ${target.h}`;
        ghost.classList.toggle("bad", !target.ok);
      };
      const onUp = () => {
        tileEl.removeEventListener("pointermove", onMove);
        tileEl.removeEventListener("pointerup", onUp);
        tileEl.removeEventListener("pointercancel", onUp);
        A.tile = t.id; A.confirmDel = false;
        if (!moved) {
          const now = Date.now();
          if (lastTap.id === t.id && now - lastTap.at < 400) { lastTap = { id: null, at: 0 }; editTile(t.id); return; }
          lastTap = { id: t.id, at: now };
          renderMain(); renderInsp(); focusTile(t.id);
          return;
        }
        const ok = target && target.ok && (target.x !== t.x || target.y !== t.y || target.w !== t.w || target.h !== t.h);
        if (ok) applyTarget(t, target);
        else if (target && !target.ok) say(resizing ? "No room for that size. Make space first, or pick a smaller size." : "No room there. Drop it on an empty spot, or on a button the same size to swap.");
        renderMain(); renderInsp();
        flash(t.id, ok ? "bump" : "nope");
      };
      tileEl.addEventListener("pointermove", onMove);
      tileEl.addEventListener("pointerup", onUp);
      tileEl.addEventListener("pointercancel", onUp);
    });
  }
  function renderMain() {
    const box = $("a-main");
    box.innerHTML = "";
    const pg = curPage();
    if (!pg) return;
    const hit = findNode(A.page);
    const bar = el("div", { className: "pagebar" });
    const crumbs = el("div", { className: "crumbs" });
    crumbs.innerHTML = `<span></span>`;
    crumbs.firstChild.textContent = [prof().name, ...hit.path.map((f) => f.name)].join(" / ") + " / ";
    crumbs.append(pg.name);
    const size = el("select");
    size.setAttribute("aria-label", "Grid size");
    const sizes = [[3, 3], [4, 3], [5, 3]];
    // A taller page (a profile's 5 × 4 or 5 × 6) keeps its own size in the list
    if (!sizes.some(([c, r]) => c === pg.cols && r === pg.rows)) sizes.push([pg.cols, pg.rows]);
    sizes.forEach(([c, r]) => size.appendChild(el("option", { value: `${c}x${r}`, textContent: `${c} × ${r} grid` })));
    size.value = `${pg.cols}x${pg.rows}`;
    size.addEventListener("change", () => {
      // Remember where every button was, change the grid, then glide each button from its old spot to its new one
      const old = $("a-grid");
      const before = new Map([...old.querySelectorAll(".tile")].map((t) => [t.dataset.id, t.getBoundingClientRect()]));
      [pg.cols, pg.rows] = size.value.split("x").map(Number);
      renderMain(); renderInsp(); renderPhones(true);
      say(`Grid is now ${pg.cols} × ${pg.rows}`);
      setTimeout(() => { const nb = $("a-main").querySelector(".pagebar .pxsel-btn"); if (nb) nb.focus(); }, 0);
      if (reduced) return;
      const g = $("a-grid"), ease = "cubic-bezier(0.2, 0.9, 0.25, 1.05)";
      g.querySelectorAll(".tile").forEach((t) => {
        const o = before.get(t.dataset.id), n = t.getBoundingClientRect();
        if (!o || !n.width) return;
        t.animate([
          { transformOrigin: "0 0", transform: `translate(${o.left - n.left}px, ${o.top - n.top}px) scale(${o.width / n.width}, ${o.height / n.height})` },
          { transformOrigin: "0 0", transform: "none" }
        ], { duration: 460, easing: ease });
      });
      g.querySelectorAll(".slot").forEach((sl, i) => sl.animate([{ opacity: 0, transform: "scale(0.85)" }, { opacity: 1, transform: "none" }], { duration: 300, delay: 160 + Math.min(i, 10) * 15, easing: ease, fill: "backwards" }));
    });
    const ed = btn(A.editLayout ? "Done" : "Edit layout", "btn sm", () => { A.editLayout = !A.editLayout; renderMain(); renderInsp(); say(A.editLayout ? "Edit layout on. Drag buttons to move them, or drag a coral corner to resize." : "Layout saved"); $("a-main").querySelector(".pagebar [aria-pressed]").focus(); });
    ed.setAttribute("aria-pressed", String(A.editLayout));
    const send = btn("Show on phones", "btn sm", () => { PHONES.forEach((p) => { if (p.online) setPhone(p, { follows: A.page }); }); say(`${PHONES.filter((p) => p.online).map((p) => p.name).join(" and ")} now show ${pg.name}`); });
    bar.append(crumbs, size, ed, send);

    const L = layoutPage(pg);
    const g = el("div", { className: `agrid${A.editLayout ? " edit" : ""}`, id: "a-grid" });
    g.style.gridTemplateColumns = `repeat(${pg.cols}, minmax(0, 1fr))`;
    g.style.gridTemplateRows = `repeat(${L.rows}, ${CELL_H}px)`;
    g.style.gap = `${GAP}px`;
    const where = findNode(pg.id);
    let back = SET.backBtn && !A.editLayout && where && where.path.length > 0;
    for (let y = 0; y < L.rows; y++) for (let x = 0; x < pg.cols; x++) {
      if (L.own.has(`${x},${y}`)) continue;
      if (back) {
        back = false;
        const parent = where.path.length > 1 ? where.path[where.path.length - 2].children : prof().tree;
        const bk = btn("‹ Back", "tile back", () => {
          const to = [];
          walk(parent, (n) => { if (n.kind === "page" && !to.length) to.push(n); });
          if (to[0]) activate(to[0]);
        }, "Back out of this folder");
        bk.style.gridColumn = `${x + 1}`;
        bk.style.gridRow = `${y + 1}`;
        g.appendChild(bk);
        continue;
      }
      const sl = btn("+", "slot", () => addTile(pg, x, y), `Add a button at column ${x + 1}, row ${y + 1}`);
      sl.style.gridColumn = `${x + 1}`;
      sl.style.gridRow = `${y + 1}`;
      g.appendChild(sl);
    }
    tilesOf(pg).forEach((t) => {
      const b = el("button", { type: "button", className: "tile" });
      b.dataset.id = t.id;
      b.style.gridColumn = `${t.x + 1} / span ${t.w}`;
      b.style.gridRow = `${t.y + 1} / span ${t.h}`;
      fillTile(b, t);
      resumeAnim(b, t);
      if (A.editLayout) {
        const rz = el("span", { className: "rz" });
        rz.setAttribute("aria-hidden", "true");
        b.appendChild(rz);
      }
      b.setAttribute("aria-label", `${tileLabel(t)}${A.editLayout ? `, column ${t.x + 1}, row ${t.y + 1}, ${t.w} by ${t.h}. Arrow keys move it, Shift and arrow keys resize it` : ""}`);
      b.setAttribute("aria-pressed", String(A.tile === t.id));
      if (A.tile === t.id) b.classList.add("sel");
      if (!A.editLayout) {
        // A click selects the button and plays its press animation right on the grid. It doesn't
        // run the action (Press in the inspector does), so editing "Lock PC" won't lock the PC.
        // The grid isn't rebuilt, so the animation (and any ON AIR loop) keeps running.
        b.addEventListener("click", () => {
          A.tile = t.id; A.confirmDel = false;
          g.querySelectorAll(".tile").forEach((x) => { const on = x === b; x.classList.toggle("sel", on); x.setAttribute("aria-pressed", String(on)); });
          // The button pushed in while the mouse held it (:active), so the click adds no second push
          renderInsp();
          // A knob is the one button a click runs: the wheel turns it, a click pushes it and a
          // right-click pulls it, as on the real panel. The push waits a moment, so the
          // double-click that opens the editor pushes nothing.
          if (t.knob && hasPlugin(t)) {
            clearTimeout(b._push);
            b._push = setTimeout(() => runTile(t).then((r) => { if (!r.ok) say(r.message); }), 260);
          }
        });
        b.addEventListener("contextmenu", (e) => {
          if (!t.knob || !hasPlugin(t)) return;
          e.preventDefault();
          playAnim(b, { type: "press" });
          runTile(t, true).then((r) => { if (!r.ok) say(r.message); });
        });
        b.addEventListener("dblclick", () => { clearTimeout(b._push); editTile(t.id); });
      } else {
        b.addEventListener("keydown", (e) => {
          const d = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] }[e.key];
          if (d) { e.preventDefault(); nudge(pg, t, d[0], d[1], e.shiftKey); }
          else if (e.key === "Enter") { e.preventDefault(); editTile(t.id); }
          else if (e.key === " ") { e.preventDefault(); A.tile = t.id; renderMain(); renderInsp(); focusTile(t.id); }
        });
      }
      g.appendChild(b);
    });
    if (A.editLayout) enableDrag(g, pg, L);
    const hint = el("p", { className: "sr-only", id: "a-grid-hint" });
    g.setAttribute("aria-describedby", "a-grid-hint");
    hint.textContent = A.editLayout
      ? "Drag a button to move it. Drop it on a button the same size to swap them. Drag the coral corner to resize. With the keyboard: arrow keys move the selected button, Shift and arrow keys resize it, Enter opens it in the editor."
      : tilesOf(pg).length ? "Select a button to see it in the inspector. Double-click to open it in the editor. Use Edit layout to move and resize buttons." : "This page is empty. Click a + to add a button.";
    box.append(bar, g, hint);
    fitGridRows(g, bar, pg, L.rows);
  }
  // Rows fill the height left above the inspector bar
  let lastFit = null;
  new ResizeObserver(() => { if (lastFit && lastFit[0].isConnected) fitGridRows(...lastFit); }).observe(document.querySelector(".deckmain"));
  function fitGridRows(g, bar, pg, rows) {
    lastFit = [g, bar, pg, rows];
    const box = $("a-main"), main = box.parentElement;
    // The grid is drawn at the last page's size first, which can add a scrollbar for a moment and
    // make the squares too small. Measure without it, unless the page has more rows than fit (it
    // really scrolls then).
    if (rows <= 3) box.style.overflowY = "hidden";
    const cw = (box.clientWidth - 20 - GAP * (pg.cols - 1)) / pg.cols;
    box.style.overflowY = "";
    // Every cell is square, like the button designer's canvas, so a design looks the same on the
    // board as where you made it: a 1 × 1 button is a square, a 2 × 1 is two squares and the gap.
    // The squares are as big as the room allows (the grid fills the width when the height lets
    // it), and the grid sits in the middle. They're sized for three rows at most: a taller page
    // keeps big buttons and scrolls.
    let h = cw > 0 ? Math.floor(cw) : 84;
    if (!window.matchMedia("(max-width: 720px)").matches && main.clientHeight) {
      const room = main.clientHeight - $("a-insp").offsetHeight - 20 - bar.offsetHeight - 10;
      const fit = Math.min(rows, 3);
      h = Math.floor(Math.min(cw, (room - GAP * (fit - 1)) / fit));
    }
    h = Math.max(48, h);
    CELL_H = h;
    g.style.gridTemplateColumns = `repeat(${pg.cols}, ${h}px)`;
    g.style.gridTemplateRows = `repeat(${rows}, ${h}px)`;
    g.style.width = `${h * pg.cols + GAP * (pg.cols - 1)}px`;
    g.style.marginInline = "auto";
  }
  function addTile(pg, x, y) {
    const t = T(uid("t-"), "", "bolt", 1, 1, "single", [newStep("key")]);
    Object.assign(t, { x, y });
    boards[pg.board].push(t);
    A.tile = t.id;
    renderMain(); renderInsp(); renderTree();
    openEditor(t.id, true);
    showEditor();
    say("New button. Set it up in the editor.");
  }
  function editTile(id) {
    openEditor(id, false);
    showEditor();
    say(`Editing ${findTile(id).label}`);
  }

