  // ===== Deck: inspector =====
  function renderInsp() {
    const box = $("a-insp");
    box.innerHTML = "";
    const pg = curPage();
    const t = A.tile && tilesOf(pg).find((x) => x.id === A.tile);
    if (!t) {
      const m = el("div");
      m.innerHTML = minnion({ c: pal("sky"), expr: "sleepy", scale: 3 });
      const showing = PHONES.filter((p) => p.online && (p.follows === A.page || (p.follows === "pc" && SET.followPc)));
      const dl = el("dl", { className: "kv" });
      [["Page", pg.name], ["Buttons", String(tilesOf(pg).length)], ["Grid", `${pg.cols} × ${pg.rows}`], ["On phones", showing.length ? showing.map((p) => p.name).join(", ") : "None"]].forEach(([k, v]) => dl.append(el("dt", { textContent: k }), el("dd", { textContent: v })));
      const a = el("div", { className: "isec" }), b = el("div", { className: "isec wide" });
      const head = el("div", { className: "ihead" });
      head.append(m, el("h3", { textContent: "Inspector" }));
      a.append(head, el("p", { className: "help", textContent: "Select a button to see what it does, which plugin runs it, and its states." }));
      b.append(el("span", { className: "cap", textContent: "This page" }), dl);
      box.append(a, b);
      return;
    }
    // The preview is the board's button exactly: drawn at the board's cell size and shape
    // (squares and the gap between them), then scaled down to fit here
    const prev = el("div", { className: "tile preview" });
    prev.setAttribute("aria-hidden", "true");
    const bw = CELL_H * t.w + GAP * (t.w - 1), bh = CELL_H * t.h + GAP * (t.h - 1);
    const k = Math.min(1, 76 / CELL_H, 156 / bw, 156 / bh);
    prev.style.width = `${bw}px`;
    prev.style.height = `${bh}px`;
    prev.style.transform = `scale(${k})`;
    const pv = el("div", { className: "ipv" });
    pv.style.width = `${Math.round(bw * k)}px`;
    pv.style.height = `${Math.round(bh * k)}px`;
    pv.appendChild(prev);
    fillTile(prev, t);
    resumeAnim(prev, t);
    const owner = tileOwner(t);
    const dl = el("dl", { className: "kv" });
    [["Animation", `${(ANIM[t.anim.type] || ANIM.press).name}${t.anim.type !== "none" ? `, ${t.anim.repeat === "loop" ? "until pressed again" : t.anim.repeat === "three" ? "3 times" : "once"}` : ""}`], ["Runs", t.mode === "sequence" ? `${t.steps.length} steps in order` : "One action"], ["Plugin", owner ? `${owner} (${pluginState(owner) === "ready" ? "ready" : STATE_WORDS[pluginState(owner)]})` : "Built in"], ["States", t.states.map((s) => s.name).join(" → ")]].forEach(([k, v]) => dl.append(el("dt", { textContent: k }), el("dd", { textContent: v })));
    const list = el("div", { className: "ilist" });
    const shown = t.steps.length > 4 ? t.steps.slice(0, 3) : t.steps;
    shown.forEach((s, i) => {
      const row = el("div", { className: "irow" });
      row.innerHTML = `${actionIcon(ACTION[s.type])}<span><span></span><small></small></span>`;
      row.querySelector("span span").textContent = `${t.steps.length > 1 ? `${i + 1}. ` : ""}${ACTION[s.type].label}`;
      row.querySelector("small").textContent = summary(s);
      list.appendChild(row);
    });
    if (shown.length < t.steps.length) list.appendChild(el("span", { className: "more", textContent: `+ ${t.steps.length - shown.length} more steps. Open the editor to see them all.` }));
    const acts = el("div", { className: "acts" });
    acts.append(
      btn("Edit button", "btn go", () => editTile(t.id)),
      // Press runs the button for real, the same as pressing it on a phone
      btn("Press", "btn", async () => {
        const tileEl = $("a-main").querySelector(`[data-id="${t.id}"]`);
        pressAnim(tileEl || prev, t);
        const r = await runTile(t);
        if (A.tile !== t.id) return;
        renderInsp();
        const np = $("a-insp").querySelector(".tile.preview");
        if (np) playAnim(np, t.anim);
        say(r.ok ? `${t.label}: ${r.message}${t.states.length > 1 ? `. Now: ${t.states[t.cur].name}` : ""}` : r.message);
      }),
      btn("Duplicate", "btn", () => {
        const c = clone(t); c.id = uid("t-"); c.label = `${t.label} copy`;
        boards[pg.board].push(c); A.tile = c.id; renderMain(); renderInsp(); renderTree();
        say(`Duplicated ${t.label}`);
      })
    );
    const move = el("select");
    move.setAttribute("aria-label", "Move to page");
    move.appendChild(el("option", { value: "", textContent: "Move to page…" }));
    pagesOf(prof()).filter(({ page }) => page.id !== pg.id).forEach(({ page, path }) => move.appendChild(el("option", { value: page.id, textContent: [...path.map((f) => f.name), page.name].join(" / ") })));
    move.addEventListener("change", () => {
      if (!move.value) return;
      const to = findNode(move.value).node;
      boards[pg.board].splice(boards[pg.board].indexOf(t), 1);
      boards[to.board].push(t);
      A.tile = null; renderMain(); renderInsp(); renderTree();
      say(`Moved ${t.label} to ${to.name}`);
    });
    const del = el("div", { className: "delrow" });
    if (!A.confirmDel) del.append(btn("Delete", "btn", () => { A.confirmDel = true; renderInsp(); $("a-del-yes").focus(); }));
    else {
      del.append(el("span", { textContent: `Delete ${t.label}?` }));
      const yes = btn("Delete", "btn go", () => { boards[pg.board].splice(boards[pg.board].indexOf(t), 1); A.tile = null; A.confirmDel = false; if (selectedId === t.id) { draft = null; selectedId = null; renderEditor(); } renderMain(); renderInsp(); renderTree(); say(`Deleted ${t.label}`); });
      yes.id = "a-del-yes";
      del.append(yes, btn("Keep it", "btn", () => { A.confirmDel = false; renderInsp(); }));
    }
    // Header: the preview, then the name with its size and the state it's in under it
    const h = el("h3"); h.textContent = t.label || "Unnamed button";
    const meta = el("p", { className: "imeta", textContent: `${t.w} × ${t.h} button · ${t.states[t.cur || 0].name}` });
    const secA = el("div", { className: "isec" }), secB = el("div", { className: "isec" }), secC = el("div", { className: "isec" });
    const head = el("div", { className: "ihead" });
    head.append(pv, el("div", { className: "iname" }, [h, meta]));
    secA.append(head, dl);
    if (A.editLayout) {
      const lay = el("div", { className: "lay" });
      const pad = el("div", { className: "nudge" });
      pad.setAttribute("role", "group");
      pad.setAttribute("aria-label", "Move button");
      const nb = (label, aria, dx, dy, col, row) => { const b = btn(label, "btn sm", () => { nudge(pg, t, dx, dy, false); }, aria); b.style.gridColumn = col; b.style.gridRow = row; return b; };
      pad.append(nb("↑", "Move up", 0, -1, "2", "1"), nb("←", "Move left", -1, 0, "1", "2"), nb("↓", "Move down", 0, 1, "2", "2"), nb("→", "Move right", 1, 0, "3", "2"));
      const sz = (label, dx, dy) => {
        const r = el("div", { className: "row" });
        r.append(el("span", { textContent: label }), btn("−", "btn sm", () => nudge(pg, t, -dx, -dy, true), `Make ${label.toLowerCase()} smaller`), btn("+", "btn sm", () => nudge(pg, t, dx, dy, true), `Make ${label.toLowerCase()} bigger`));
        return r;
      };
      lay.append(el("span", { className: "cap", textContent: "Move" }), pad, el("span", { className: "cap", textContent: "Size" }), el("div", { className: "stack" }, [sz("Width", 1, 0), sz("Height", 0, 1)]));
      secB.append(el("span", { className: "cap", textContent: `Column ${t.x + 1}, row ${t.y + 1}` }), lay);
    }
    else secB.append(el("span", { className: "cap", textContent: t.steps.length > 1 ? "Steps" : "Action" }), list);
    if (A.confirmDel) secC.append(acts, del);
    else { acts.append(move, ...del.childNodes); acts.lastChild.classList.add("del"); secC.append(acts); }
    box.append(secA, secB, secC);
  }

