  // ===== Deck: inspector =====
  // A slim bar under the grid: the selected button's name, what it does and its size on the left,
  // Edit button, Press and Duplicate on the right. "⋯" swaps in Move to page and Delete.
  // With Edit layout on, the right side moves and resizes the button instead.
  let moreFor = null;  // the button whose "⋯" is open
  function renderInsp() {
    const box = $("a-insp");
    box.innerHTML = "";
    const pg = curPage();
    const t = A.tile && tilesOf(pg).find((x) => x.id === A.tile);
    const txt = el("div", { className: "itxt" });
    const h = el("h3");
    const meta = el("p", { className: "imeta" });
    txt.append(h, meta);
    const acts = el("div", { className: "acts" });
    box.append(txt, acts);
    if (!t) {
      const showing = PHONES.filter((p) => p.online && (p.follows === A.page || (p.follows === "pc" && SET.followPc)));
      const n = tilesOf(pg).length;
      h.textContent = pg.name;
      meta.textContent = [`${n} button${n === 1 ? "" : "s"}`, `${pg.cols} × ${pg.rows} grid`, `On phones: ${showing.length ? showing.map((p) => p.name).join(", ") : "none"}`].join(" · ");
      acts.appendChild(el("p", { className: "help", textContent: n ? "Select a button to see it here. Double-click one to edit it." : "Click a + to add a button." }));
      return;
    }
    const owner = tileOwner(t);
    const first = t.steps[0] && ACTION[t.steps[0].type];
    const does = t.mode === "sequence" ? `${t.steps.length} steps in order` : first ? first.label : "No action";
    const bits = [does, `${t.w} × ${t.h} button`];
    if (t.states.length > 1) bits.push(t.states[t.cur || 0].name);
    if (owner && pluginState(owner) !== "ready") bits.push(`${owner} is ${STATE_WORDS[pluginState(owner)]}`);
    h.textContent = t.label || "Unnamed button";
    meta.textContent = bits.join(" · ");
    meta.title = meta.textContent;

    if (A.editLayout) {
      // Move and size buttons in one row: ← ↑ ↓ → and Width −/+, Height −/+
      meta.textContent = `Column ${t.x + 1}, row ${t.y + 1} · ${t.w} × ${t.h} button`;
      const pad = el("div", { className: "nudge" });
      pad.setAttribute("role", "group");
      pad.setAttribute("aria-label", "Move button");
      [["←", "Move left", -1, 0], ["↑", "Move up", 0, -1], ["↓", "Move down", 0, 1], ["→", "Move right", 1, 0]].forEach(([l, a, dx, dy]) => pad.appendChild(btn(l, "btn sm", () => nudge(pg, t, dx, dy, false), a)));
      const sz = (label, dx, dy) => {
        const r = el("div", { className: "row" });
        r.append(el("span", { textContent: label }), btn("−", "btn sm", () => nudge(pg, t, -dx, -dy, true), `Make ${label.toLowerCase()} smaller`), btn("+", "btn sm", () => nudge(pg, t, dx, dy, true), `Make ${label.toLowerCase()} bigger`));
        return r;
      };
      acts.classList.add("lay");
      acts.append(pad, sz("Width", 1, 0), sz("Height", 0, 1));
      return;
    }

    if (moreFor === t.id) {
      // Move to page, Delete (it asks first), and back to the main buttons
      const move = el("select");
      move.setAttribute("aria-label", "Move to page");
      move.appendChild(el("option", { value: "", textContent: "Move to page…" }));
      pagesOf(prof()).filter(({ page }) => page.id !== pg.id).forEach(({ page, path }) => move.appendChild(el("option", { value: page.id, textContent: [...path.map((f) => f.name), page.name].join(" / ") })));
      move.addEventListener("change", () => {
        if (!move.value) return;
        const to = findNode(move.value).node;
        boards[pg.board].splice(boards[pg.board].indexOf(t), 1);
        boards[to.board].push(t);
        A.tile = null; moreFor = null; renderMain(); renderInsp(); renderTree();
        say(`Moved ${t.label} to ${to.name}`);
      });
      if (!A.confirmDel) {
        acts.append(move, btn("Delete", "btn", () => { A.confirmDel = true; renderInsp(); $("a-del-yes").focus(); }), btn("Back", "btn", () => { moreFor = null; renderInsp(); }, "Back to Edit button, Press and Duplicate"));
      } else {
        meta.textContent = `Delete ${t.label}? This can't be undone.`;
        const yes = btn("Delete", "btn go", () => { boards[pg.board].splice(boards[pg.board].indexOf(t), 1); A.tile = null; A.confirmDel = false; moreFor = null; if (selectedId === t.id) { draft = null; selectedId = null; renderEditor(); } renderMain(); renderInsp(); renderTree(); say(`Deleted ${t.label}`); });
        yes.id = "a-del-yes";
        acts.append(yes, btn("Keep it", "btn", () => { A.confirmDel = false; renderInsp(); }));
      }
      return;
    }

    acts.append(
      btn("Edit button", "btn go", () => editTile(t.id)),
      // Press runs the button for real, the same as pressing it on a phone
      btn("Press", "btn", async () => {
        const tileEl = $("a-main").querySelector(`[data-id="${t.id}"]`);
        if (tileEl) pressAnim(tileEl, t);
        const r = await runTile(t);
        if (A.tile !== t.id) return;
        renderInsp();
        say(r.ok ? `${t.label}: ${r.message}${t.states.length > 1 ? `. Now: ${t.states[t.cur].name}` : ""}` : r.message);
      }),
      btn("Duplicate", "btn", () => {
        const c = clone(t); c.id = uid("t-"); c.label = `${t.label} copy`;
        boards[pg.board].push(c); A.tile = c.id; renderMain(); renderInsp(); renderTree();
        say(`Duplicated ${t.label}`);
      }),
      btn("⋯", "btn more", () => { moreFor = t.id; A.confirmDel = false; renderInsp(); const f = $("a-insp").querySelector(".pxsel-btn, select"); if (f) f.focus(); }, "More: move to another page, or delete")
    );
  }

