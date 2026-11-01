  // ===== Phones =====
  // Paired phones and pairing come from the backend (src-tauri/src/phones/). The phone app is
  // not made yet, so phones can't connect yet; everything on the PC side is real.

  // The backend's phone entry, in the shape the page uses
  const ago = (ms) => {
    const s = Math.max(0, (Date.now() - ms) / 1000);
    if (s < 60) return "just now";
    if (s < 3600) return `${Math.round(s / 60)} min ago`;
    if (s < 86400) return `${Math.round(s / 3600)} h ago`;
    const d = Math.round(s / 86400);
    return d === 1 ? "yesterday" : `${d} days ago`;
  };
  const phoneEntry = (p) => ({ ...p, ver: p.appVersion || "unknown", link: "Wi-Fi", seen: p.lastSeenMs ? ago(p.lastSeenMs) : "never" });
  function setPhones(list) {
    PHONES.splice(0, PHONES.length, ...(list || []).map(phoneEntry));
    if (!PHONES.some((p) => p.id === previewPhone)) previewPhone = PHONES[0] ? PHONES[0].id : null;
  }
  // Change how a phone shows pages. The backend saves it; the list comes back through phones_changed.
  function setPhone(p, patch) {
    Object.assign(p, patch);
    backend.run("update_phone", { id: p.id, patch }).catch((e) => say(`Could not change ${p.name}: ${e}`));
  }

  // ----- Pairing -----
  // A code is asked for while the Phones page is open. It works once and lasts 5 minutes.
  const PAIR = { info: null, loading: false, ended: "", server: null };
  async function newPair() {
    if (PAIR.loading || !backend.available()) return;
    PAIR.loading = true;
    try {
      PAIR.server = await backend.run("phone_server_status");
      PAIR.info = await backend.run("start_pairing");
      PAIR.ended = "";
    } catch (e) { PAIR.info = null; PAIR.ended = `Could not make a pairing code: ${e}`; }
    PAIR.loading = false;
    renderPhones(true);
  }
  function endPairing() {
    if (PAIR.info) backend.call("cancel_pairing");
    PAIR.info = null;
    PAIR.ended = "";
  }
  const pairLeft = () => (PAIR.info ? Math.max(0, PAIR.info.expiresMs - Date.now()) : 0);
  const mmss = (ms) => { const s = Math.ceil(ms / 1000); return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`; };
  // Keep the countdown moving without redrawing the page
  setInterval(() => {
    if (A.view !== "phones" || !PAIR.info) return;
    const left = pairLeft();
    const t = $("pair-exp");
    if (left <= 0) { PAIR.info = null; PAIR.ended = "This code has expired. Get a new code to pair a phone."; renderPhones(true); }
    else if (t) t.textContent = `Works once. Expires in ${mmss(left)}.`;
  }, 1000);
  function onPairingChanged(e) {
    if (e.reason === "cancelled") return;
    PAIR.info = null;
    if (e.reason === "paired") { PAIR.ended = ""; say(`${e.phone || "A phone"} is paired`); if (A.view === "phones") newPair(); return; }
    PAIR.ended = e.reason === "too_many_tries" ? "Too many wrong codes were tried. Get a new code." : "This code has expired. Get a new code to pair a phone.";
    renderPhones(true);
  }

  // The QR code from the backend, drawn as pixels like the rest of the app
  function qrSVG(qr) {
    let r = "";
    for (let y = 0; y < qr.size; y++) for (let x = 0; x < qr.size; x++) if (qr.modules[y * qr.size + x] === "1") r += `<rect x="${x}" y="${y}" width="1" height="1"/>`;
    return `<svg viewBox="-2 -2 ${qr.size + 4} ${qr.size + 4}" width="132" height="132" shape-rendering="crispEdges" aria-hidden="true" style="fill:#1e2433">${r}</svg>`;
  }
  const formatCode = (c) => c.replace(/(\d{3})(\d{3})/, "$1 $2");

  function pairCard() {
    const pair = el("div", { className: "pair" });
    const qr = el("div", { className: "qr" });
    const pinfo = el("div", { className: "stack" });
    pinfo.style.gap = "8px";
    const ol = el("ol", { className: "steps-list" });
    ["Install Minniondeck on the phone and open it.", "Tap Add a PC.", "Scan this code, or type the number."].forEach((t) => ol.appendChild(el("li", { textContent: t })));
    pinfo.append(el("b", { textContent: "Pair a new phone" }), ol);
    const info = PAIR.info;
    if (!backend.available()) {
      pinfo.append(el("p", { className: "help", textContent: "Pairing works in the Minniondeck desktop app." }));
    } else if (info) {
      qr.innerHTML = qrSVG(info.qr);
      qr.setAttribute("role", "img");
      qr.setAttribute("aria-label", "QR code for pairing a phone");
      const where = info.addresses.length ? `Or type this PC's address: ${info.addresses[0]}, port ${info.port}.` : "This PC isn't on a local network. Connect it to Wi-Fi, then get a new code.";
      pinfo.append(el("div", { className: "pcode", textContent: formatCode(info.code) }),
        el("p", { className: "help", id: "pair-exp", textContent: `Works once. Expires in ${mmss(pairLeft())}.` }),
        el("p", { className: "help", textContent: `${where}${SET.pin ? " The phone shows the same number so you can check it." : ""}` }));
    } else {
      pinfo.append(el("p", { className: "help", textContent: PAIR.ended || "Making a pairing code…" }));
    }
    if (backend.available()) pinfo.append(btn("New code", "btn sm", newPair));
    pair.append(qr, pinfo);
    return pair;
  }
  // Ready for phones, or why not
  function serverLine() {
    const s = PAIR.server;
    if (!s) return null;
    if (s.error) return el("p", { className: "warn", textContent: s.error });
    return el("p", { className: "help", textContent: `Ready for phones on this network (port ${s.port}). Paired phones connect by themselves when the Minniondeck phone app opens.` });
  }

  const followName = (p) => { if (p.follows === "pc") return "Follows this PC"; const h = findPageAny(p.follows); return h ? h.node.name : "Follows this PC"; };
  function renderPhones(soft) {
    if (soft && A.view !== "phones") return;
    if (A.view === "phones" && !PAIR.info && !PAIR.ended && !PAIR.loading) newPair();
    const box = $("av-phones");
    const active = document.activeElement && box.contains(document.activeElement) ? document.activeElement.id : null;
    box.innerHTML = "";
    const pad = el("div", { className: "vpad" });
    const head = el("div", { className: "vhead" });
    head.append(el("h3", { textContent: "Phones" }));
    pad.appendChild(head);
    const cols = el("div", { className: "phones" });
    const left = el("div", { className: "stack" });
    const list = el("div", { className: "devlist" });
    if (!PHONES.length) list.appendChild(el("p", { className: "muted", textContent: "No phones paired yet. Pair one with the code below." }));
    PHONES.forEach((p) => {
      const d = el("div", { className: `dev${p.online ? "" : " off"}` });
      const ico = el("div", { className: "dev-ico" });
      ico.innerHTML = glyph("phone", p.kind === "tablet" ? 44 : 36);
      const info = el("div");
      const title = el("b", { textContent: p.name });
      const meta = el("small");
      meta.innerHTML = `<span class="dot ${p.online ? "ok" : ""}"></span>`;
      const os = p.os ? ` · ${p.os}` : "";
      meta.append(p.online ? `Connected over ${p.link}${os} · app ${p.ver}${p.battery != null ? ` · battery ${p.battery}%` : ""}` : `Offline, last seen ${p.seen}${os} · app ${p.ver}`);
      const row = el("div", { className: "row" });
      const sid = `follow-${p.id}`;
      const s = el("select", { id: sid });
      s.setAttribute("aria-label", `${p.name} shows`);
      s.appendChild(el("option", { value: "pc", textContent: "Follows this PC" }));
      PROFILES.forEach((q) => pagesOf(q).forEach(({ page }) => s.appendChild(el("option", { value: page.id, textContent: `${q.name} / ${page.name}` }))));
      s.value = findPageAny(p.follows) ? p.follows : "pc";
      s.addEventListener("change", () => { setPhone(p, { follows: s.value }); renderPhones(); });
      row.append(s);
      if (removing === `ph-${p.id}`) {
        row.append(el("span", { textContent: `Remove ${p.name}? Its key stops working at once.` }),
          btn("Remove", "btn go", async () => {
            removing = null;
            try { await backend.run("forget_phone", { id: p.id }); say(`Removed ${p.name}`); } catch (e) { say(`Could not remove ${p.name}: ${e}`); }
          }),
          btn("Keep", "btn", () => { removing = null; renderPhones(); }));
      } else {
        row.append(btn("Preview", "btn sm", () => { previewPhone = p.id; renderPhones(); }, `Preview ${p.name}`), btn("Remove", "btn sm", () => { removing = `ph-${p.id}`; renderPhones(); }, `Remove ${p.name}`));
      }
      info.append(title, meta, row);
      d.append(ico, info);
      list.appendChild(d);
    });
    const line = serverLine();
    left.append(el("span", { className: "cap", textContent: "Paired phones" }), list, ...(line ? [line] : []), pairCard(), updatePanel());
    cols.append(left, phonePreview());
    pad.appendChild(cols);
    box.appendChild(pad);
    // The phone app isn't out yet: the page stays, under a white cover that says so
    pad.inert = true;
    box.classList.add("soon");
    box.append(el("div", { className: "soon-cover" }, [el("b", { textContent: "Coming soon" }), el("p", { textContent: "The Minniondeck phone app is on its way." })]));
    if (active && $(active)) $(active).focus();
  }
  // Phone app updates arrive with the update source (a GitHub repo, later). Until then this says so.
  function updatePanel() {
    const u = el("div", { className: "upd" });
    u.id = "a-upd";
    const h = el("div", { className: "vhead" });
    h.append(el("h3", { textContent: "Phone app updates" }));
    const push = btn("Push update to phones", "btn go", null);
    push.id = "a-push";
    push.disabled = true;
    u.append(h, el("p", { className: "help", textContent: "No update source is set up yet. Once the phone app is released, its newest version shows here and you can tell your phones to update. Phones download updates themselves." }), el("div", { className: "row" }, [push]));
    return u;
  }
  // The phone shows the page's grid filling its whole screen. In portrait the grid can turn on its side
  // so buttons stay close to square: the PC's columns become the phone's rows.
  let rotateFrom = 0;
  function phonePreview() {
    const p = PHONES.find((x) => x.id === previewPhone) || PHONES[0];
    const wrap = el("div", { className: "stack pv" });
    if (!p) return wrap;
    const pageId = p.follows === "pc" ? A.page : p.follows;
    const hit = findPageAny(pageId) || { node: curPage(), prof: prof() };
    const pg = hit.node;
    const land = p.orient === "landscape";
    // Phones always show the page's own grid, the same as the PC (an older phone setting for a
    // grid of its own is ignored). In portrait it can turn on its side.
    const turn = !land && p.fit === "turn";
    const L = layoutPage(pg);
    const C = turn ? L.rows : pg.cols, R = turn ? pg.cols : L.rows;

    // Controls: orientation, and how the grid fits in portrait
    const ctr = el("div", { className: "row" });
    const seg = (label, opts, val, fn) => {
      const sgEl = el("div", { className: "seg" });
      sgEl.setAttribute("role", "group");
      sgEl.setAttribute("aria-label", label);
      opts.forEach(([v, l]) => { const b = btn(l, "", () => fn(v)); b.setAttribute("aria-pressed", String(v === val)); sgEl.appendChild(b); });
      return sgEl;
    };
    ctr.append(seg(`${p.name} orientation`, [["portrait", "Portrait"], ["landscape", "Landscape"]], p.orient, (v) => {
      if (v === p.orient) return;
      rotateFrom = v === "landscape" ? 90 : -90;
      setPhone(p, { orient: v });
      renderPhones();
      $("av-phones").querySelector(".pv .seg button[aria-pressed='true']").focus();
    }));
    if (!land) ctr.append(seg("Grid in portrait", [["turn", "Turn grid"], ["keep", "Keep columns"]], p.fit, (v) => { setPhone(p, { fit: v }); renderPhones(); $("av-phones").querySelector(".pv .seg + .seg button[aria-pressed='true']").focus(); }));
    wrap.append(el("span", { className: "cap", textContent: `${p.name} preview` }), ctr);

    // Screen size from the phone's real shape, scaled to fit the space here
    // The preview column is 300px wide; on a phone-width screen it spans the view
    const narrow = window.matchMedia("(max-width: 720px)").matches;
    const room = narrow ? Math.max(220, ($("av-phones").clientWidth || 360) - 40) : 288;
    const long = p.kind === "tablet" ? 420 : 460;
    const short = Math.round(long / p.aspect);
    const sw = land ? long : short, sh = land ? short : long;
    const ph = el("div", { className: `phone${land ? " land" : ""}${p.kind === "tablet" ? " tab" : ""}` });
    ph.setAttribute("aria-label", `Preview of ${p.name}, ${p.orient}`);
    ph.setAttribute("role", "group");
    const screen = el("div", { className: "pscreen" });
    screen.style.width = `${sw}px`;
    screen.style.height = `${sh}px`;
    const bar = el("div", { className: "pbar" });
    bar.append(el("span", { textContent: `${hit.prof.name} / ${pg.name}` }), el("span", { textContent: p.online && p.battery != null ? `${p.battery}%` : "" }));
    const g = el("div", { className: "pgrid" });
    // Landscape goes nearly full screen: a thinner bar and no page dots
    const GAPP = land || p.kind === "tablet" ? 6 : 5, PAD = 6, BAR = land ? 24 : 28, NAV = land ? 0 : 22;
    screen.style.gridTemplateRows = `${BAR}px minmax(0, 1fr) ${NAV}px`;
    g.style.gap = `${GAPP}px`;
    g.style.padding = `${PAD}px`;
    const cw = (sw - PAD * 2 - GAPP * (C - 1)) / C, ch = (sh - BAR - NAV - PAD * 2 - GAPP * (R - 1)) / R;
    // Square cells: the biggest square that fits both ways, so the grid fills the tighter side
    // of the screen for any grid size, and sits centred along the other side
    const cell = Math.max(8, Math.floor(Math.min(cw, ch)));
    g.style.gridTemplateColumns = `repeat(${C}, ${cell}px)`;
    g.style.gridTemplateRows = `repeat(${R}, ${cell}px)`;
    g.style.justifyContent = "center";
    g.style.alignContent = "center";
    const icon = Math.max(16, Math.min(48, Math.floor((cell * 0.42) / 8) * 8));
    const fs = Math.max(10, Math.min(14, Math.round(cell / 6)));
    const showText = cell >= 40;
    // Empty cells show as dashed squares, like the empty slots on the board
    for (let y = 0; y < L.rows; y++) for (let x = 0; x < pg.cols; x++) {
      if (L.own.has(`${x},${y}`)) continue;
      const e = el("i", { className: "pempty" });
      e.style.gridColumn = `${(turn ? y : x) + 1}`;
      e.style.gridRow = `${(turn ? x : y) + 1}`;
      g.appendChild(e);
    }
    // The buttons, where they are on the PC
    tilesOf(pg).forEach((t) => {
      const b = el("button", { type: "button", className: "ptile" });
      const [cx, cy, cwSpan, chSpan] = turn ? [t.y, t.x, t.h, t.w] : [t.x, t.y, t.w, t.h];
      b.style.gridColumn = `${cx + 1} / span ${cwSpan}`;
      b.style.gridRow = `${cy + 1} / span ${chSpan}`;
      b.dataset.id = t.id;
      b.dataset.icon = String(icon);
      // A turned button shows the other size's face (2 × 1 becomes 1 × 2)
      b.dataset.fw = String(cwSpan);
      b.dataset.fh = String(chSpan);
      b.dataset.text = showText ? "1" : "0";
      b.style.fontSize = `${fs}px`;
      paintPtile(b, t);
      b.setAttribute("aria-label", `${t.label} on ${p.name}`);
      resumeAnim(b, t, "phone");
      b.addEventListener("click", () => { pressAnim(b, t, "phone"); advanceTile(t, el("div")); say(`${p.name} pressed ${t.label}`); });
      g.appendChild(b);
    });
    const nav = el("div", { className: "pnav" });
    nav.setAttribute("aria-hidden", "true");
    pagesOf(hit.prof).forEach(({ page }) => nav.appendChild(el("i", { className: page.id === pg.id ? "on" : "" })));
    nav.hidden = land;
    screen.append(bar, g, nav);
    if (!p.online) {
      const o = el("div", { className: "pover" });
      o.innerHTML = minnion({ c: pal("sky"), expr: "sleepy", scale: 4 });
      o.append(el("span", { textContent: `${p.name} is offline` }), el("span", { className: "help", textContent: `Last seen ${p.seen}. It shows these buttons once the phone app opens on it.` }));
      screen.appendChild(o);
    }
    const notch = el("div", { className: "notch" });
    ph.append(notch, screen);
    const info = turn
      ? `Turned to fit: the ${pg.cols} × ${L.rows} grid shows as ${C} × ${R}. Each square button is ${cell} × ${cell} px here.`
      : `${C} × ${R} grid filling the screen. Each square button is ${cell} × ${cell} px here.`;
    // Draw the phone at its true size, then scale the whole frame down if the space is narrow
    const holder = el("div", { className: "phold" });
    holder.appendChild(ph);
    const note = el("p", { className: "help", textContent: `${p.name} · app ${p.ver}. ${info}` });
    wrap.append(holder, note);
    const fit = () => {
      if (!holder.isConnected) return;
      const fw = ph.offsetWidth, fh = ph.offsetHeight;
      const k = Math.min(1, room / fw);
      ph.style.transform = k < 1 ? `scale(${k})` : "";
      holder.style.width = `${Math.round(fw * k)}px`;
      holder.style.height = `${Math.round(fh * k)}px`;
      if (k < 1) note.textContent = `${p.name} · app ${p.ver}. ${info} Shown smaller to fit here.`;
    };
    requestAnimationFrame(fit);
    if (rotateFrom && !reduced) {
      const deg = rotateFrom;
      requestAnimationFrame(() => holder.animate([{ transform: `rotate(${deg}deg)` }, { transform: "rotate(0deg)" }], { duration: 240, easing: "steps(4, end)" }));
    }
    rotateFrom = 0;
    return wrap;
  }

  // Phone tiles redraw in place so a running animation is not cut off
  function paintPtile(b, t) {
    const st = t.states[t.cur || 0];
    const icon = Number(b.dataset.icon) || 24;
    // Text only keeps its text even on small phone buttons; it's the whole point of the button
    const textOnly = t.style.shape === "text";
    const iconOnly = !textOnly && (t.style.shape === "icon" || b.dataset.text === "0");
    const gp = gifParts(st, iconOnly ? Math.min(56, icon + 8) : icon);
    const text = withLive(t, st, gp, iconOnly ? Math.min(56, icon + 8) : icon);
    const sb = stateBg(st);
    // A live map fills the button, as on the board
    b.classList.toggle("has-map", !!screenDataOf(t));
    if (screenDataOf(t)) { b.style.background = ""; b.innerHTML = screenHTML(t.id); requestAnimationFrame(() => drawMaps(t.id)); return; }
    b.style.background = sb.css;
    b.style.color = textOn(st.bg);
    b.classList.toggle("has-dsg", !!sb.design);
    b.classList.toggle("has-img", sb.pic);
    if (sb.design) { b.innerHTML = designHTML(sb.design, Number(b.dataset.fw) || t.w, Number(b.dataset.fh) || t.h); b.classList.remove("has-gif", "has-wdg"); return; }
    b.innerHTML = `${sb.under}${gp.bg}${textOnly ? "" : gp.icon}${gp.wdg}${iconOnly ? "" : '<span class="pl"></span>'}`;
    if (!iconOnly) b.querySelector(".pl").textContent = text;
    b.classList.toggle("text-only", textOnly);
    b.classList.toggle("has-gif", !!gp.bg);
    b.classList.toggle("has-wdg", !!gp.wdg);
  }
  // One button can show in three places at once. After a press, redraw the others and match their loop.
  window.syncTileEls = (t, except) => {
    document.querySelectorAll(`#grid [data-id="${t.id}"], #a-grid [data-id="${t.id}"], .ptile[data-id="${t.id}"]`).forEach((e) => {
      const phone = e.classList.contains("ptile");
      if (e !== except) {
        if (phone) paintPtile(e, t);
        else {
          fillTile(e, t);
          if (e.closest("#grid")) { const h = el("span", { className: "handle" }); h.dataset.resize = "1"; h.setAttribute("aria-hidden", "true"); e.appendChild(h); }
          if (e.closest("#a-grid.edit")) { const rz = el("span", { className: "rz" }); rz.setAttribute("aria-hidden", "true"); e.appendChild(rz); }
        }
      }
      const where = phone ? "phone" : "pc";
      if (t._looping && !e.dataset.loop) resumeAnim(e, t, where);
      if (!t._looping && e.dataset.loop) stopAnim(e);
    });
  };

