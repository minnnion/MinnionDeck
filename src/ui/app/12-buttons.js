  // ===== Running buttons =====
  // A button's action runs on this PC (src-tauri/src/actions.rs) when it is pressed on a phone,
  // or with Press in the inspector. Clicking a button on the Deck only selects it.

  // Actions that reach outside Minniondeck. Imported profiles must be allowed once before these run.
  const OUTSIDE = new Set(["key", "text", "click", "run", "url"]);
  // What runs on the PC: every step but turning pages, which Minniondeck does itself
  const stepsOf = (t) => (t.steps || []).filter((s) => !NAV_STEPS.has(s.type)).map((s) => ({ kind: s.type, fields: s.f || {} }));
  const navOf = (t) => (t.steps || []).find((s) => NAV_STEPS.has(s.type)) || null;

  // ----- Turning pages (Go to page, Back) -----
  // The pages opened here, newest last, for Back
  const trail = [];
  let trailAt = "";
  setInterval(() => {
    const key = A.prof && A.page ? `${A.prof}/${A.page}` : "";
    if (!key || key === trailAt) return;
    if (trailAt) trail.push(trailAt);
    if (trail.length > 40) trail.shift();
    trailAt = key;
  }, 200);
  // A folder's first page
  const firstPageIn = (node) => {
    if (!node) return null;
    if (node.kind === "page") return node;
    for (const c of node.children || []) { const p = firstPageIn(c); if (p) return p; }
    return null;
  };
  function openPage(profId, pageId) {
    if (trailAt && trailAt !== `${profId}/${pageId}`) { trail.push(trailAt); if (trail.length > 40) trail.shift(); }
    A.prof = profId; A.page = pageId; A.tile = null; A.confirmDel = false;
    trailAt = `${profId}/${pageId}`;
    if (A.view === "deck") { renderTree(); renderMain(); renderInsp(); renderCrumb(); renderStatus(); }
  }
  // Turn the page a button says; a message for its result
  function turnPage(step) {
    if (step.type === "back") {
      while (trail.length) {
        const [profId, pageId] = trail.pop().split("/");
        const h = findPageAny(pageId);
        if (h && h.prof.id === profId && h.node.kind === "page") {
          A.prof = profId; A.page = pageId; A.tile = null; A.confirmDel = false;
          trailAt = `${profId}/${pageId}`;
          if (A.view === "deck") { renderTree(); renderMain(); renderInsp(); renderCrumb(); renderStatus(); }
          return `Back to ${h.node.name}`;
        }
      }
      // Nothing before it: the profile's first page
      const first = pagesOf(prof())[0];
      if (!first) throw new Error("There is no page to go back to.");
      openPage(A.prof, first.page.id);
      return `Back to ${first.page.name}`;
    }
    const h = findPageAny((step.f || {}).page);
    const page = h && firstPageIn(h.node);
    if (!page) throw new Error("The page this button goes to isn't there any more. Choose another in Edit button.");
    openPage(h.prof.id, page.id);
    return `Page: ${page.name}`;
  }
  // The pages and folders of the profile open now, for Go to page's list
  window.pageChoices = () => {
    const out = [];
    const walkTree = (nodes, path) => (nodes || []).forEach((n) => {
      const name = [...path, n.name].join(" / ");
      out.push({ value: n.id, label: n.kind === "folder" ? `${name} (folder)` : name });
      if (n.kind === "folder") walkTree(n.children, [...path, n.name]);
    });
    walkTree((prof() || {}).tree, []);
    return out;
  };
  const ctxOf = (t) => ({ id: t.id, state: t.cur || 0 });
  const hasPlugin = (t) => (t.steps || []).some((s) => String(s.type).startsWith("plug:"));
  // A plugin action with its own states: the plugin moves the button between them, not each press
  const ownsStates = (t) => (t.steps || []).some((s) => ACTION[s.type] && ACTION[s.type].ownStates);
  const profileOfTile = (id) => PROFILES.find((p) => pagesOf(p).some(({ page }) => tilesOf(page).some((t) => t.id === id))) || null;
  function tileById(id) {
    for (const key of Object.keys(boards)) { const t = boards[key].find((x) => x.id === id); if (t) return t; }
    return null;
  }

  async function allowed(p, t) {
    if (!p || !p.imported || p.trusted || !(t.steps || []).some((s) => OUTSIDE.has(s.type))) return true;
    const ok = await backend.call("confirm_imported", { name: p.name }, false);
    if (ok) { p.trusted = true; flush(); }
    return !!ok;
  }

  // Run a button and, if it worked, move it to its next state. Resolves to { ok, message }.
  // A long press (from a phone) on a plugin button goes to the plugin's long_press instead.
  async function runTile(t, long = false) {
    const p = profileOfTile(t.id);
    if (!(await allowed(p, t))) return { ok: false, message: `Buttons in ${p.name} aren't allowed to open programs or press keys yet. Press one on the PC to allow them.` };
    const owner = tileOwner(t);
    if (owner && pluginState(owner) !== "ready") return { ok: false, message: `Could not run ${t.label}. ${owner} is ${STATE_WORDS[pluginState(owner)]}.` };
    const nav = navOf(t);
    // Only turning a page: nothing runs on the PC
    if (nav && !stepsOf(t).length) {
      try { return { ok: true, message: turnPage(nav) }; } catch (e) { return { ok: false, message: `${t.label}: ${e.message || e}` }; }
    }
    if (!backend.available()) { advanceTile(t, el("div")); return { ok: true, message: "pressed (actions run in the desktop app)" }; }
    try {
      let message = long && hasPlugin(t)
        ? await backend.run("plugin_event", { event: "long_press", button: ctxOf(t), steps: stepsOf(t) })
        : await backend.run("run_button", { steps: stepsOf(t), button: ctxOf(t) });
      if (!ownsStates(t)) advanceTile(t, el("div"));
      // The other steps ran: then the page turns
      if (nav) message = turnPage(nav);
      return { ok: true, message };
    } catch (e) {
      return { ok: false, message: `${t.label}: ${e}` };
    } finally {
      // Save now, so phones see the button's new state straight away
      flush();
    }
  }

  // A live map on a button is looking at something (zoomed in on an airport, say): its plugin
  // hears it and sends what's there. Maps on the PC call this; maps on phones send "view".
  window.mapLooksAt = (buttonId, data) => {
    const t = tileById(buttonId);
    if (t && hasPlugin(t)) backend.call("plugin_event", { event: "view", button: ctxOf(t), steps: stepsOf(t), data });
  };
  backend.listen("phone_view", (e) => window.mapLooksAt(e.buttonId, e.data));

  // Knob buttons (t.knob, a plugin action with "knob": true) turn: with the mouse wheel on the
  // Deck, a drag on a phone. Wheel up turns clockwise. Notches close together go as one turn, so
  // a quick spin is a quick turn.
  const turning = new Map();
  // Each click turns the knob's picture 15 degrees, with no end, so it reads like the real knob
  const KNOB_STEP = 15;
  function turnKnob(t, by) {
    if (!t || !t.knob || !hasPlugin(t) || !by) return;
    backend.call("plugin_event", { event: "turn", button: ctxOf(t), steps: stepsOf(t), data: { by } });
    // Not wrapped at 360: 345 to 0 would spin the picture back the long way
    t._angle = (t._angle || 0) + by * KNOB_STEP;
    document.querySelectorAll(`[data-id="${t.id}"]`).forEach((el) => knobAngle(el, t));
  }
  document.addEventListener("wheel", (e) => {
    const el = e.target.closest && e.target.closest("#a-main [data-id]");
    const t = el && tileById(el.dataset.id);
    if (!t || !t.knob) return;
    e.preventDefault();
    const cur = turning.get(t.id) || { by: 0 };
    cur.by += e.deltaY < 0 ? 1 : -1;
    if (!cur.timer) cur.timer = setTimeout(() => { turning.delete(t.id); turnKnob(t, cur.by); }, 60);
    turning.set(t.id, cur);
  }, { passive: false });
  backend.listen("phone_turn", (e) => turnKnob(tileById(e.buttonId), Math.max(-50, Math.min(50, Math.round(Number(e.by) || 0)))));

  // A button pressed on a phone: run it here, show it on the Deck, and tell the phone how it went
  backend.listen("phone_press", async (e) => {
    const reply = (ok, message) => backend.call("phone_press_result", { phoneId: e.phoneId, buttonId: e.buttonId, ok, message });
    const t = tileById(e.buttonId);
    if (!t) { if (e.kind !== "release") reply(false, "That button no longer exists."); return; }
    // The finger lifted: only plugins care, and the phone waits for nothing
    if (e.kind === "release") {
      if (hasPlugin(t)) backend.call("plugin_event", { event: "release", button: ctxOf(t), steps: stepsOf(t) });
      return;
    }
    const tileEl = $("a-main").querySelector(`[data-id="${t.id}"]`);
    if (tileEl && A.view === "deck") pressAnim(tileEl, t);
    const r = await runTile(t, e.kind === "long");
    reply(r.ok, r.message);
    if (!r.ok) say(`${e.phoneName}: ${r.message}`);
    if (A.view === "deck") { renderInsp(); renderTree(); }
  });

  // Plugins hear when their buttons come on screen and leave it: the page open on the PC, and the
  // pages the phones show (a phone says which when it turns a page), so a phone on another page
  // gets its buttons' live values too. A button whose action changes counts as leaving and coming back.
  let shownNow = new Map();
  let phonePages = [];
  backend.listen("phone_pages", (pages) => { phonePages = Array.isArray(pages) ? pages : []; });
  // A phone turned a page: the PC opens it too, so the two stay together (and phones that follow
  // the PC turn with it; the one that turned is already there)
  backend.listen("phone_page", (e) => {
    const h = e && findPageAny(e.page);
    if (!h || h.node.kind !== "page" || (A.prof === h.prof.id && A.page === h.node.id)) return;
    A.prof = h.prof.id; A.page = h.node.id; A.tile = null; A.confirmDel = false;
    if (A.view === "deck") { renderTree(); renderMain(); renderInsp(); renderCrumb(); renderStatus(); }
  });
  // A page and the 5 before and 5 after it, in its profile's order: their buttons are watched too,
  // so a swipe shows live values at once rather than after the sim answers
  const NEAR = 5;
  function nearPages(id) {
    const h = findPageAny(id);
    if (!h || h.node.kind !== "page") return [];
    const list = pagesOf(h.prof).map((x) => x.page);
    const i = list.findIndex((p) => p.id === id);
    return list.slice(Math.max(0, i - NEAR), i + NEAR + 1);
  }
  function resetShown() { shownNow = new Map(); }
  setInterval(() => {
    if (!backend.available()) return;
    const ids = [...new Set([A.prof && A.page ? A.page : null, ...phonePages].filter(Boolean))];
    const pages = [...new Set(ids.flatMap(nearPages))];
    const now = new Map();
    pages.forEach((p) => tilesOf(p).forEach((t) => { if (hasPlugin(t) && !now.has(t.id)) now.set(t.id, { button: ctxOf(t), steps: stepsOf(t), key: JSON.stringify(t.steps) }); }));
    shownNow.forEach((old, id) => {
      const n = now.get(id);
      if (!n || n.key !== old.key) backend.call("plugin_event", { event: "hidden", button: old.button, steps: old.steps });
    });
    now.forEach((n, id) => {
      const old = shownNow.get(id);
      if (!old || old.key !== n.key) backend.call("plugin_event", { event: "shown", button: n.button, steps: n.steps });
    });
    shownNow = now;
  }, 500);

  // Phones that follow this PC turn to the page open here
  let pcPageSent = "";
  setInterval(() => {
    if (!SET.followPc || !A.prof || !A.page) return;
    const key = `${A.prof}/${A.page}`;
    if (key === pcPageSent) return;
    pcPageSent = key;
    backend.call("set_pc_page", { profile: A.prof, page: A.page });
  }, 300);

  // Settings > Profiles and pages > Switch profiles automatically. The backend says which app is
  // in front (src-tauri/src/foreground.rs); the profile that names it is opened.
  const APP_EXES = {
    "OBS Studio": ["obs64.exe", "obs32.exe", "obs.exe"],
    "Microsoft Teams": ["ms-teams.exe", "teams.exe"],
    "Steam": ["steam.exe", "steamwebhelper.exe"],
    "Spotify": ["spotify.exe"],
    "Visual Studio Code": ["code.exe"]
  };
  const KNOWN_EXES = Object.values(APP_EXES).flat();
  function profileForApp(front) {
    return PROFILES.find((p) => {
      if (!p.auto || p.auto === "Never") return false;
      if (p.auto === "Any full-screen game") return front.fullscreen && !KNOWN_EXES.includes(front.exe);
      return (APP_EXES[p.auto] || []).includes(front.exe);
    }) || null;
  }
  backend.listen("app_in_front", (front) => {
    if (!SET.autoSwitch || !front) return;
    const p = profileForApp(front);
    if (!p || p.id === A.prof) return;
    switchProfile(p.id);
    if (A.view === "profiles") renderProfiles();
  });
