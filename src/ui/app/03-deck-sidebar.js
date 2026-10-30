  // ===== Deck: sidebar =====
  function renderDeck() { renderProfBtn(); renderTree(); renderMain(); renderInsp(); }
  function renderProfBtn() {
    const box = $("a-prof");
    box.innerHTML = "";
    const p = prof();
    const b = el("button", { type: "button", className: "pick-btn", id: "a-prof-btn" });
    b.setAttribute("aria-haspopup", "true");
    b.setAttribute("aria-expanded", String(A.profMenu));
    b.innerHTML = `${chipFor(p)}<span class="pick-txt"><span></span><small></small></span>${glyph("caret", 16)}`;
    b.querySelector(".pick-txt span").textContent = p.name;
    b.querySelector(".pick-txt small").textContent = SET.autoSwitch && p.auto && p.auto !== "Never" ? `Switches on with ${p.auto}` : "Switched by hand";
    b.setAttribute("aria-label", `Profile: ${p.name}`);
    b.addEventListener("click", () => { A.profMenu = !A.profMenu; renderProfBtn(); if (A.profMenu) box.querySelector(".menu button").focus(); });
    box.appendChild(b);
    if (!A.profMenu) return;
    const menu = el("div", { className: "menu" });
    menu.setAttribute("role", "menu");
    PROFILES.forEach((q) => {
      const it = el("button", { type: "button" });
      it.setAttribute("role", "menuitemradio");
      it.setAttribute("aria-checked", String(q.id === A.prof));
      it.innerHTML = `${chipFor(q)}<span></span>`;
      it.lastChild.textContent = q.name;
      it.addEventListener("click", () => { switchProfile(q.id); $("a-prof-btn").focus(); });
      menu.appendChild(it);
    });
    menu.appendChild(el("hr"));
    const nw = el("button", { type: "button", textContent: "+ New profile" }); nw.setAttribute("role", "menuitem");
    nw.addEventListener("click", () => { newProfile(); go("profiles"); });
    const mg = el("button", { type: "button", textContent: "Manage profiles" }); mg.setAttribute("role", "menuitem");
    mg.addEventListener("click", () => go("profiles"));
    menu.append(nw, mg);
    menu.addEventListener("keydown", (e) => {
      const items = [...menu.querySelectorAll("button")];
      const i = items.indexOf(document.activeElement);
      if (e.key === "ArrowDown") items[(i + 1) % items.length].focus();
      else if (e.key === "ArrowUp") items[(i - 1 + items.length) % items.length].focus();
      else if (e.key === "Escape") { A.profMenu = false; renderProfBtn(); $("a-prof-btn").focus(); }
      else return;
      e.preventDefault();
    });
    box.appendChild(menu);
  }
  document.addEventListener("pointerdown", (e) => { if (A.profMenu && !e.target.closest("#a-prof")) { A.profMenu = false; renderProfBtn(); } });
  function switchProfile(id) {
    A.prof = id;
    A.profMenu = false;
    A.page = pagesOf(prof())[0].page.id;
    A.tile = null;
    renderDeck(); renderStatus();
    say(`Profile: ${prof().name}`);
  }

  function renderTree() {
    const ul = $("a-tree");
    ul.innerHTML = "";
    const build = (nodes, parent) => {
      nodes.forEach((n) => {
        const li = el("li");
        li.setAttribute("role", "treeitem");
        li.dataset.id = n.id;
        li.tabIndex = -1;
        if (n.id === A.freshNode && !reduced) li.classList.add("fresh");
        const node = el("div", { className: "node" });
        if (n.kind === "folder") {
          li.setAttribute("aria-expanded", String(n.open));
          node.innerHTML = `<span class="tw">${glyph("caret", 12)}</span>${glyph("folder", 18)}<span class="nm"></span><span class="cnt"></span>`;
          node.querySelector(".cnt").textContent = `${n.children.length}`;
        } else {
          li.setAttribute("aria-selected", String(n.id === A.page));
          node.innerHTML = `<span class="tw"></span>${glyph("grid", 16)}<span class="nm"></span><span class="cnt"></span>`;
          node.querySelector(".cnt").textContent = `${tilesOf(n).length}`;
        }
        if (A.renaming === n.id) {
          const inp = el("input", { value: n.name, maxLength: 24 });
          inp.setAttribute("aria-label", `Rename ${n.name}`);
          const done = (keep) => { if (A.renaming !== n.id) return; if (keep && inp.value.trim()) n.name = inp.value.trim(); A.renaming = null; renderTree(); renderMain(); renderCrumb(); ul.querySelector(`[data-id="${n.id}"]`).focus(); };
          inp.addEventListener("keydown", (e) => { e.stopPropagation(); if (e.key === "Enter") { e.preventDefault(); done(true); } if (e.key === "Escape") done(false); });
          inp.addEventListener("blur", () => done(true));
          node.querySelector(".nm").replaceWith(inp);
        } else {
          node.querySelector(".nm").textContent = n.name;
        }
        li.setAttribute("aria-label", n.kind === "folder" ? `${n.name}, folder, ${n.children.length} items` : `${n.name}, ${tilesOf(n).length} buttons`);
        node.addEventListener("click", () => activate(n));
        node.addEventListener("dblclick", () => { A.renaming = n.id; renderTree(); });
        li.appendChild(node);
        if (n.kind === "folder") {
          const g = el("ul");
          g.setAttribute("role", "group");
          g.hidden = !n.open;
          if (!n.children.length) { const empty = el("li", { className: "note", textContent: "Empty. Add a page while this folder is open." }); empty.setAttribute("role", "none"); g.appendChild(empty); }
          build(n.children, g);
          li.appendChild(g);
        }
        parent.appendChild(li);
      });
    };
    build(prof().tree, ul);
    A.freshNode = null;
    const sel = ul.querySelector('[aria-selected="true"]') || ul.querySelector("[role=treeitem]");
    if (sel) sel.tabIndex = 0;
    const inp = ul.querySelector("input");
    if (inp) { inp.focus(); inp.select(); }
  }
  function activate(n) {
    if (n.kind === "folder") { n.open = !n.open; A.lastFolder = n.id; renderTree(); }
    else { A.page = n.id; A.tile = null; A.confirmDel = false; renderTree(); renderMain(); renderInsp(); renderCrumb(); renderStatus(); }
    const li = $("a-tree").querySelector(`[data-id="${n.id}"]`);
    if (li) { $("a-tree").querySelectorAll("[role=treeitem]").forEach((x) => { x.tabIndex = -1; }); li.tabIndex = 0; li.focus(); }
  }
  $("a-tree").addEventListener("keydown", (e) => {
    const li = e.target.closest("[role=treeitem]");
    if (!li || e.target.tagName === "INPUT") return;
    const visible = [...$("a-tree").querySelectorAll("[role=treeitem]")].filter((x) => !x.parentElement.closest("[hidden]"));
    const i = visible.indexOf(li);
    const n = findNode(li.dataset.id).node;
    const focus = (x) => { if (!x) return; visible.forEach((v) => { v.tabIndex = -1; }); x.tabIndex = 0; x.focus(); };
    if (e.key === "ArrowDown") focus(visible[i + 1]);
    else if (e.key === "ArrowUp") focus(visible[i - 1]);
    else if (e.key === "Home") focus(visible[0]);
    else if (e.key === "End") focus(visible[visible.length - 1]);
    else if (e.key === "ArrowRight" && n.kind === "folder") { if (!n.open) activate(n); else focus(visible[i + 1]); }
    else if (e.key === "ArrowLeft") { if (n.kind === "folder" && n.open) activate(n); else focus(li.parentElement.closest("[role=treeitem]")); }
    else if (e.key === "Enter" || e.key === " ") activate(n);
    else if (e.key === "F2") { A.renaming = n.id; renderTree(); }
    else return;
    e.preventDefault();
  });
  // ----- Moving pages and folders: drag one onto a folder to put it inside, above or below another
  // item to put it there (in that item's folder or at the top level), or below the list to put it
  // at the top level. With the keyboard: Alt + Up or Down moves it, Alt + Left takes it out of its
  // folder, Alt + Right puts it in the folder just above it.
  const listOf = (id) => { const hit = findNode(id); return hit.path.length ? hit.path[hit.path.length - 1].children : prof().tree; };
  const holds = (folder, id) => { let found = false; walk(folder.children, (x) => { if (x.id === id) found = true; }); return found; };
  function moveNode(id, where, targetId) {
    const n = findNode(id).node;
    if (targetId && (targetId === id || (n.kind === "folder" && holds(n, targetId)))) return false;
    const from = listOf(id);
    from.splice(from.indexOf(n), 1);
    if (where === "end") prof().tree.push(n);
    else if (where === "in") { const f = findNode(targetId).node; f.children.push(n); f.open = true; }
    else { const list = listOf(targetId), t = findNode(targetId).node; list.splice(list.indexOf(t) + (where === "after" ? 1 : 0), 0, n); }
    A.freshNode = n.id;
    return true;
  }
  function afterMove(n, where, targetId) {
    renderTree(); renderCrumb();
    const hit = findNode(n.id);
    const place = hit.path.length ? `in ${hit.path[hit.path.length - 1].name}` : "at the top level";
    say(`Moved ${n.name} ${where === "in" ? `into ${findNode(targetId).node.name}` : place}`);
    const li = $("a-tree").querySelector(`[data-id="${n.id}"]`);
    if (li) { $("a-tree").querySelectorAll("[role=treeitem]").forEach((x) => { x.tabIndex = -1; }); li.tabIndex = 0; li.focus(); }
  }
  let treeDragged = false;
  $("a-tree").addEventListener("pointerdown", (e) => {
    const nodeEl = e.target.closest(".node");
    if (!nodeEl || e.button !== 0 || e.target.tagName === "INPUT") return;
    const li = nodeEl.closest("[role=treeitem]"), id = li.dataset.id, n = findNode(id).node;
    const sx = e.clientX, sy = e.clientY, tree = $("a-tree");
    let dragging = false, ghost = null, drop = null;
    const clear = () => { tree.querySelectorAll(".drop-in, .drop-before, .drop-after").forEach((x) => x.classList.remove("drop-in", "drop-before", "drop-after")); tree.classList.remove("drop-end"); };
    const mv = (ev) => {
      if (!dragging) {
        if (Math.hypot(ev.clientX - sx, ev.clientY - sy) < 6) return;
        dragging = true;
        li.classList.add("dragging");
        ghost = el("div", { className: "tree-ghost" });
        ghost.innerHTML = `${glyph(n.kind === "folder" ? "folder" : "grid", 16)}<span></span>`;
        ghost.lastChild.textContent = n.name;
        document.body.appendChild(ghost);
      }
      ghost.style.left = `${ev.clientX + 14}px`;
      ghost.style.top = `${ev.clientY + 10}px`;
      clear();
      drop = null;
      const under = document.elementFromPoint(ev.clientX, ev.clientY);
      const tNode = under && under.closest("#a-tree .node");
      if (tNode) {
        const tli = tNode.closest("[role=treeitem]"), tid = tli.dataset.id, t = findNode(tid).node;
        if (tid === id || (n.kind === "folder" && holds(n, tid))) return;
        const r = tNode.getBoundingClientRect(), f = (ev.clientY - r.top) / r.height;
        const where = t.kind === "folder" && f > 0.25 && f < 0.75 ? "in" : f < 0.5 ? "before" : "after";
        drop = { where, tid };
        tli.classList.add(`drop-${where}`);
        return;
      }
      // Under the last item: the top level
      const tr = tree.getBoundingClientRect();
      if (ev.clientX >= tr.left && ev.clientX <= tr.right && ev.clientY > tr.bottom - 2 && ev.clientY < tr.bottom + 80) { drop = { where: "end" }; tree.classList.add("drop-end"); }
    };
    const up = () => {
      document.removeEventListener("pointermove", mv);
      document.removeEventListener("pointerup", up);
      document.removeEventListener("pointercancel", up);
      if (ghost) ghost.remove();
      li.classList.remove("dragging");
      clear();
      if (!dragging) return;
      treeDragged = true;
      setTimeout(() => { treeDragged = false; }, 0);
      if (drop && moveNode(id, drop.where, drop.tid)) afterMove(n, drop.where, drop.tid);
    };
    document.addEventListener("pointermove", mv);
    document.addEventListener("pointerup", up);
    document.addEventListener("pointercancel", up);
  });
  // A drag ends with a click on the item; it shouldn't also open or fold it
  $("a-tree").addEventListener("click", (e) => { if (treeDragged) { e.stopPropagation(); e.preventDefault(); } }, true);
  $("a-tree").addEventListener("keydown", (e) => {
    const li = e.target.closest("[role=treeitem]");
    if (!li || !e.altKey || e.target.tagName === "INPUT" || !["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"].includes(e.key)) return;
    e.preventDefault();
    e.stopImmediatePropagation();
    const hit = findNode(li.dataset.id), n = hit.node, list = listOf(n.id), i = list.indexOf(n);
    let ok = false, where = null, target = null;
    if (e.key === "ArrowUp" && i > 0) { where = "before"; target = list[i - 1].id; }
    else if (e.key === "ArrowDown" && i < list.length - 1) { where = "after"; target = list[i + 1].id; }
    else if (e.key === "ArrowLeft" && hit.path.length) { where = "after"; target = hit.path[hit.path.length - 1].id; }
    else if (e.key === "ArrowRight" && i > 0 && list[i - 1].kind === "folder") { where = "in"; target = list[i - 1].id; }
    if (where) ok = moveNode(n.id, where, target);
    if (ok) afterMove(n, where, target); else say(e.key === "ArrowLeft" ? `${n.name} is already at the top level` : e.key === "ArrowRight" ? "Alt + Right puts it in the folder just above it" : `${n.name} can't move further`);
  }, true);
  function targetList() {
    const hit = findNode(A.page);
    const folderId = A.lastFolder && findNode(A.lastFolder) && findNode(A.lastFolder).node.open ? A.lastFolder : hit && hit.path.length ? hit.path[hit.path.length - 1].id : null;
    return folderId ? findNode(folderId).node.children : prof().tree;
  }
  $("a-add-page").addEventListener("click", () => {
    const key = uid("Board");
    boards[key] = [];
    const pg = P(uid("p-"), "New page", key);
    targetList().push(pg);
    A.page = pg.id; A.tile = null; A.freshNode = pg.id; A.renaming = pg.id;
    renderDeck(); renderCrumb();
    say("New page added. Type a name, then press Enter.");
  });
  $("a-add-folder").addEventListener("click", () => {
    const f = F(uid("f-"), "New folder", true, []);
    prof().tree.push(f);
    A.lastFolder = f.id; A.freshNode = f.id; A.renaming = f.id;
    renderTree();
    say("New folder added. Type a name, then press Enter.");
  });

