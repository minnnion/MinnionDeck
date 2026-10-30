  // ===== Saving, export and import =====
  // The backend writes the files (see src-tauri/src/storage.rs). This side decides what is
  // saved, notices changes, and gathers or merges a profile for .mdprofile export and import.
  //
  // Two parts are saved from here: the library (profiles, pages, buttons, designs, GIFs, pictures,
  // crew) and the Settings page. With no saved data yet, the built-in examples are kept and become
  // the first save. Paired phones are kept by the backend itself (phones/hub.rs).

  const SAVE = { on: false, mediaDir: "", seen: {}, sent: {}, changed: 0, since: 0, busy: null, lastError: "" };
  // What goes wrong here goes in the app's log too: saving once stopped for a day with nothing
  // said, and an error in a timer or at start is seen by nobody. At most 20 errors a run.
  const uiLog = (text) => backend.call("ui_log", { text: String(text) });
  let logged = 0;
  const logError = (where, e) => { if (logged++ < 20) uiLog(`${where}: ${(e && e.stack) || e}`); };
  window.addEventListener("error", (e) => logError("error", e.error || e.message));
  window.addEventListener("unhandledrejection", (e) => logError("unhandled", e.reason));
  // Something the content security policy (tauri.conf.json) stopped: a picture or font that didn't
  // load, or markup that tried to run script
  window.addEventListener("securitypolicyviolation", (e) => logError("blocked by the security policy", `${e.effectiveDirective} ${e.blockedURI || "(inline)"} ${e.sourceFile || ""}:${e.lineNumber || ""}`));
  const PARTS = ["library", "settings"];
  // How long to wait after the last change, and the longest a change can wait while you keep editing
  const QUIET_MS = 2000, MAX_WAIT_MS = 10000, TICK_MS = 1000;

  // JSON without the UI's own working fields (names starting with "_", such as _looping)
  // A button a plugin moved to a state (a lamp lit) keeps its saved state: the plugin's is live,
  // and saving it would send phones a whole layout for every lamp
  const str = (v) => JSON.stringify(v, function (k, x) {
    if (k === "cur" && this && "_savedCur" in this) return this._savedCur;
    return k.charAt(0) === "_" ? undefined : x;
  });
  const mediaUrl = (file) => backend.mediaUrl(SAVE.mediaDir, file);
  // Pictures and GIF files are saved without their picture data; the file name points at it
  const mediaEntry = ({ src, ...m }) => m;
  const drawnEntry = (g) => ({ id: g.id, name: g.name, size: g.w, fps: g.fps, frames: g.frames });
  const dropGif = (g) => { delete GIF[g.id]; const i = GIFS.indexOf(g); if (i >= 0) GIFS.splice(i, 1); };
  const idNumber = (id) => parseInt(String(id).replace(/^\D+/, ""), 10) || 0;

  function builtinGifsInUse() {
    const ids = new Set();
    Object.values(boards).flat().forEach((t) => t.states.forEach((st) => { if (st.gif) ids.add(st.gif.id); if (st.bgGif) ids.add(st.bgGif.id); }));
    DESIGNS.forEach((d) => allFaces(d).forEach((f) => { if (f.bg && f.bg.gif) ids.add(f.bg.gif); (f.layers || []).forEach((l) => { if (l.gif) ids.add(l.gif); }); }));
    return [...ids].map((id) => GIF[id]).filter(isBuiltinGif);
  }
  function librarySnapshot() {
    // The designs buttons use, found in one pass (asking each design in turn went through every
    // button once per design: seconds of work with a thousand designs)
    const used = new Set();
    Object.values(boards).forEach((list) => list.forEach((t) => t.states.forEach((st) => { if (st.bgType === "design" && st.design) used.add(st.design); })));
    return {
      profiles: PROFILES,
      boards,
      designs: DESIGNS.filter((d) => !d.own || used.has(d.id)),
      drawnGifs: USER_GIFS.map(drawnEntry),
      // Built-in GIFs the buttons and designs use, as frames, for phones (not read back on load)
      phoneGifs: builtinGifsInUse().map(builtinGifEntry),
      gifFiles: IMPORTED_GIFS.filter((g) => g.media).map(mediaEntry),
      images: USER_IMAGES.filter((m) => m.media).map(mediaEntry),
      crew: crew.map(({ name, body, tall }) => ({ name, body, tall })),
      recentColors,
      active: { prof: A.prof, page: A.page }
    };
  }
  const parts = () => ({ library: str(librarySnapshot()), settings: str(SET) });

  // ----- Loading -----
  function applyLibrary(d) {
    if (!Array.isArray(d.profiles) || !d.profiles.length || !d.boards || typeof d.boards !== "object") throw new Error("The saved profiles are not complete.");
    PROFILES.splice(0, PROFILES.length, ...d.profiles);
    Object.keys(boards).forEach((k) => delete boards[k]);
    Object.assign(boards, d.boards);
    DESIGNS.splice(0, DESIGNS.length, ...(d.designs || []));
    DESIGNS.forEach(ensureFaces);
    USER_GIFS.splice(0).forEach(dropGif);
    (d.drawnGifs || []).forEach((s) => { const g = makeUserGif(s.name, s.size, s.frames, s.fps); delete GIF[g.id]; g.id = s.id; GIF[g.id] = g; });
    ugSeq = Math.max(0, ...USER_GIFS.map((g) => idNumber(g.id)));
    IMPORTED_GIFS.splice(0).forEach(dropGif);
    (d.gifFiles || []).forEach((s) => { const g = { ...s, src: mediaUrl(s.media) }; IMPORTED_GIFS.push(g); GIFS.push(g); GIF[g.id] = g; });
    USER_IMAGES.splice(0, USER_IMAGES.length, ...(d.images || []).map((m) => ({ ...m, src: mediaUrl(m.media) })));
    if (Array.isArray(d.crew) && d.crew.length) { crew.splice(0, crew.length, ...d.crew.map((c) => ({ ...c, expr: "open", look: 0 }))); nextRunner = 0; }
    recentColors.splice(0, recentColors.length, ...(d.recentColors || []));
    layerSeq = Math.max(layerSeq, ...DESIGNS.flatMap((x) => allFaces(x).flatMap((f) => (f.layers || []).map((l) => idNumber(l.id)))));
    // Open on start: the page chosen in Settings, otherwise where you left off
    const act = d.active || {};
    const start = (SET.startPage !== "last" && findPageAny(SET.startPage)) || findPageAny(act.page);
    if (start) { A.prof = start.prof.id; A.page = start.node.id; }
    else { A.prof = PROFILES[0].id; const first = pagesOf(PROFILES[0])[0]; A.page = first ? first.page.id : null; }
  }
  function applySettings(s) {
    Object.keys(SET).forEach((k) => { if (k in s) SET[k] = s[k]; });
    // Earlier versions saved the example page as the start page without being asked
    if (SET.startPage === "p-main") SET.startPage = "last";
    // USB isn't available yet, so phones always connect over Wi-Fi
    SET.conn = "wifi";
    // Backups to keep is 1, 3 or 5; older versions offered 10 and 30
    if (!["1", "3", "5"].includes(String(SET.keep))) SET.keep = "5";
  }
  // Returns any warnings to show. A part that can't be applied stops auto-save, so the file
  // on disk is never replaced by a half-loaded copy.
  async function loadSaved() {
    const s = await backend.call("load_state", {}, null);
    if (!s) return { warnings: [], ok: true };
    SAVE.mediaDir = s.mediaDir;
    const warnings = [...(s.warnings || [])];
    try {
      // Settings first: "Open on start" decides which page the library opens on
      if (s.settings) applySettings(JSON.parse(s.settings));
      if (s.library) applyLibrary(JSON.parse(s.library));
    } catch (e) {
      console.error("[save] could not load saved data:", e);
      logError("load", e);
      warnings.unshift("Your saved profiles could not be loaded, so nothing will be saved this time. Your files are unchanged.");
      return { warnings, ok: false };
    }
    return { warnings, ok: true };
  }

  // ----- Auto-save -----
  // Store new pictures and GIF files once, as files. The picture data stays in memory for this session.
  async function storeMedia() {
    for (const m of [...USER_IMAGES, ...IMPORTED_GIFS]) {
      if (!m.media && typeof m.src === "string" && m.src.startsWith("data:")) m.media = await backend.run("save_media", { id: m.id, dataUrl: m.src });
    }
  }
  // The library is looked at for changes only after something could have changed it: you used the
  // window (a click, a key, a drop), or a button ran (runTile saves at once). Making the save text
  // means writing the whole library out (megabytes with big profiles), so doing it every second
  // whatever happened made the whole window stutter. Every LOOK_MS it's looked at anyway.
  const LOOK_MS = 30000;
  let touched = true, lookedAt = 0;
  ["pointerup", "keyup", "change", "input", "drop", "wheel"].forEach((ev) => document.addEventListener(ev, () => { touched = true; }, true));
  function tick() {
    if (!SAVE.on || SAVE.busy) return;
    const due = Date.now() - lookedAt >= LOOK_MS;
    if (!touched && !SAVE.since && !due) return;
    touched = false;
    lookedAt = Date.now();
    let p;
    // Making the save can fail (a value that can't be written as JSON): said once, logged, and
    // tried again each second, rather than failing quietly every second
    try { p = parts(); } catch (e) {
      const msg = `Could not save: ${e.message || e}`;
      if (msg !== SAVE.lastError) { SAVE.lastError = msg; say(msg); logError("save", e); }
      return;
    }
    const now = Date.now();
    if (PARTS.some((k) => p[k] !== SAVE.seen[k])) { SAVE.seen = p; SAVE.changed = now; if (!SAVE.since) SAVE.since = now; }
    if (SAVE.since && (now - SAVE.changed >= QUIET_MS || now - SAVE.since >= MAX_WAIT_MS)) flush();
  }
  // Write anything not yet saved, now. Used by the timer, and before close, export, import and restore.
  async function flush() {
    if (!SAVE.on) return;
    while (SAVE.busy) await SAVE.busy;
    SAVE.busy = (async () => {
      try {
        await storeMedia();
        const p = parts();
        const args = {};
        PARTS.forEach((k) => { if (p[k] !== SAVE.sent[k]) args[k] = p[k]; });
        SAVE.seen = p; SAVE.since = 0;
        if (Object.keys(args).length) { await backend.run("save_state", args); Object.assign(SAVE.sent, args); }
        SAVE.lastError = "";
      } catch (e) {
        console.warn("[save]", e);
        SAVE.since = Date.now(); // try again after the next quiet spell
        const msg = `Could not save: ${e}`;
        if (msg !== SAVE.lastError) { SAVE.lastError = msg; say(msg); logError("save", e); }
      } finally {
        SAVE.busy = null;
      }
    })();
    return SAVE.busy;
  }
  window.mdFlush = flush;
  // Settings > General > When you close the window: Keep running (to the tray) or Quit
  window.mdCloseToTray = () => SET.close !== "quit";

  // ----- Export -----
  // Everything one profile needs: its pages' buttons, and the designs, GIFs and pictures they use
  function profileBundle(p) {
    const pageBoards = {}, designIds = new Set(), gifIds = new Set(), imageIds = new Set(), needed = new Set();
    const useGif = (id) => { if (id && id !== "none") gifIds.add(id); };
    pagesOf(p).forEach(({ page }) => { pageBoards[page.board] = tilesOf(page); });
    Object.values(pageBoards).flat().forEach((t) => {
      (t.steps || []).forEach((s) => { const a = ACTION[s.type]; if (a && a.plugin) needed.add(a.plugin); });
      (t.states || []).forEach((st) => {
        // An icon or GIF from a plugin needs that plugin on the other PC too
        [st.glyph, st.gif && st.gif.id, st.bgGif && st.bgGif.id].forEach((id) => {
          if (typeof id !== "string" || !id.startsWith("plug:")) return;
          const q = plugins.find((x) => x.id === id.split(":")[1]);
          needed.add(q ? q.name : id.split(":")[1]);
        });
        if (st.bgImage && st.bgImage.src) imageIds.add(st.bgImage.src);
        if (st.bgGif) useGif(st.bgGif.id);
        if (st.gif) useGif(st.gif.id);
        if (st.design) designIds.add(st.design);
      });
    });
    const designs = DESIGNS.filter((d) => designIds.has(d.id));
    designs.forEach((d) => allFaces(d).forEach((f) => {
      if (f.bg && f.bg.image) imageIds.add(f.bg.image);
      if (f.bg) useGif(f.bg.gif);
      (f.layers || []).forEach((l) => { if (l.image) imageIds.add(l.image); useGif(l.gif); });
    }));
    const { imported, trusted, needs, ...profile } = p;
    return {
      profile, boards: pageBoards, designs,
      drawnGifs: USER_GIFS.filter((g) => gifIds.has(g.id)).map(drawnEntry),
      gifFiles: IMPORTED_GIFS.filter((g) => gifIds.has(g.id) && g.media).map(mediaEntry),
      images: USER_IMAGES.filter((m) => imageIds.has(m.id) && m.media).map(mediaEntry),
      plugins: [...needed]
    };
  }
  async function exportProfile(p) {
    try {
      await flush();
      await storeMedia();
      const path = await backend.run("export_profile", { name: p.name, bundle: str(profileBundle(p)) });
      if (path) say(`Exported ${p.name} to ${path}`);
    } catch (e) { say(`Could not export ${p.name}: ${e}`); }
  }
  async function exportAllProfiles() {
    try {
      await flush();
      await storeMedia();
      const r = await backend.run("export_all_profiles", { profiles: PROFILES.map((p) => ({ name: p.name, bundle: str(profileBundle(p)) })) });
      if (r) say(`Exported ${r.count} ${r.count === 1 ? "profile" : "profiles"} to ${r.folder}`);
    } catch (e) { say(`Could not export the profiles: ${e}`); }
  }

  // ----- Import -----
  // Add an imported profile with new IDs for everything, so nothing already here is replaced
  function mergeImported(b) {
    const ref = new Map();
    const re = (id) => (id && ref.has(id) ? ref.get(id) : id);
    (b.drawnGifs || []).forEach((s) => { const g = makeUserGif(s.name, s.size || 16, s.frames || [], s.fps || 8); ref.set(s.id, g.id); });
    (b.gifFiles || []).forEach((s) => { const g = { ...s, id: `f${IMPORTED_GIFS.length + 1}${uid("")}`, src: mediaUrl(s.media) }; IMPORTED_GIFS.push(g); GIFS.push(g); GIF[g.id] = g; ref.set(s.id, g.id); });
    (b.images || []).forEach((s) => { const m = { ...s, id: `u:${uid("")}`, src: mediaUrl(s.media) }; USER_IMAGES.push(m); ref.set(s.id, m.id); });
    (b.designs || []).forEach((s) => {
      const d = clone(s);
      d.id = uid("d");
      ensureFaces(d);
      allFaces(d).forEach((f) => {
        if (f.bg) { if (f.bg.image) f.bg.image = re(f.bg.image); if (f.bg.gif) f.bg.gif = re(f.bg.gif); }
        (f.layers || []).forEach((l) => { l.id = `l${++layerSeq}`; if (l.image) l.image = re(l.image); if (l.gif) l.gif = re(l.gif); });
      });
      DESIGNS.push(d);
      ref.set(s.id, d.id);
    });
    let pages = 0, buttons = 0;
    const made = [];
    const tree = (nodes) => nodes.map((n) => {
      if (n.kind === "folder") { const f = F(uid("f-"), n.name || "Folder", n.open !== false, tree(n.children || [])); ref.set(n.id, f.id); return f; }
      pages++;
      const key = uid("Board");
      made.push(key);
      boards[key] = (n.buttons || []).map((t) => {
        buttons++;
        const c = clone(t);
        c.id = uid("t-");
        (c.states || []).forEach((st) => {
          if (st.bgImage) st.bgImage.src = re(st.bgImage.src);
          if (st.bgGif) st.bgGif.id = re(st.bgGif.id);
          if (st.gif) st.gif.id = re(st.gif.id);
          if (st.design) st.design = re(st.design);
        });
        return c;
      });
      const page = P(uid("p-"), n.name || "Page", key, n.cols || 4, n.rows || 3);
      ref.set(n.id, page.id);
      return page;
    });
    const src = b.profile || {};
    const base = (src.name || "Imported profile").slice(0, 24);
    let name = base, k = 2;
    while (PROFILES.some((q) => q.name === name)) name = `${base.slice(0, 20)} ${k++}`;
    const p = { id: uid("pr-"), name, body: src.body || "peach", icon: src.icon || "star", auto: src.auto || "Never", tree: tree(src.tree || []) };
    // Go to page buttons follow their page or folder to its new ID
    made.forEach((key) => boards[key].forEach((t) => (t.steps || []).forEach((s) => { if (s.type === "page" && s.f) s.f.page = re(s.f.page); })));
    if (!pagesOf(p).length) { const key = uid("Board"); boards[key] = []; p.tree.push(P(uid("p-"), "Main", key)); }
    // Marked so buttons that open programs or press keys can ask once before their first run
    Object.assign(p, { imported: true, trusted: false, needs: b.plugins || [] });
    PROFILES.push(p);
    return { p, pages, buttons };
  }
  async function importProfileFile(path) {
    try {
      await flush();
      const b = JSON.parse(await backend.run("import_profile", { path }));
      const { p, pages, buttons } = mergeImported(b);
      keepMissingPluginActions();
      A.freshCard = p.id;
      go("profiles");
      say(`Imported ${p.name}: ${pages} ${pages === 1 ? "page" : "pages"}, ${buttons} ${buttons === 1 ? "button" : "buttons"}`);
      flush();
    } catch (e) { say(`Could not import that file: ${e}`); }
  }
  async function pickAndImport() {
    try {
      const path = await backend.run("pick_profile_file");
      if (path) await importProfileFile(path);
    } catch (e) { say(`${e}`); }
  }
  // A file opened with Minniondeck (double-click, or handed over at start): a profile is
  // imported; a plugin is shown in Settings › Plugins with an Install button. A marketplace
  // install link (minniondeck://install?id=…) opens the install popup.
  function openFile(path) {
    if (/^minniondeck:/i.test(path)) return openInstallLink(path);
    return /\.mdplugin$/i.test(path) ? offerPlugin(path) : importProfileFile(path);
  }
  window.mdOpenProfileFile = openFile;

  // Plugins an imported profile needs, and whether each one is here yet
  function pluginNeeds(p) {
    if (!p.needs || !p.needs.length) return null;
    const words = p.needs.map((n) => `${n} (${pluginState(n) === "missing" ? "not installed" : "installed"})`);
    return el("p", { className: "help", textContent: `Uses plugins: ${words.join(", ")}` });
  }

  // ----- Backups (Settings → Backup) -----
  let restoring = null;
  function backupPanel() {
    const box = el("div", { className: "stack" });
    box.style.gap = "8px";
    const status = el("p", { className: "help", textContent: "Loading backups…" });
    const w = el("div", { className: "table-wrap" });
    const now = btn("Back up now", "btn sm", async () => {
      try { await flush(); const name = await backend.run("backup_now"); say(name ? "Backed up your profiles" : "Nothing to back up yet"); renderSettings(); }
      catch (e) { say(`Could not back up: ${e}`); }
    });
    box.append(el("b", { textContent: "Restore a backup" }), status, w, el("div", { className: "row" }, [now]));
    backend.call("list_backups", {}, null).then((list) => {
      if (!list) { status.textContent = "Backups are made by the Minniondeck desktop app."; return; }
      status.textContent = list.length
        ? "A backup is made when Minniondeck starts, and before every import or restore. The newest 5 are kept."
        : "No backups yet. One is made each time Minniondeck starts, and before every import or restore.";
      if (!list.length) return;
      const t = el("table");
      t.innerHTML = "<thead><tr><th>Made</th><th>Size</th><th></th></tr></thead>";
      const tb = el("tbody");
      list.forEach((b) => {
        const tr = el("tr");
        const when = new Date(b.createdMs).toLocaleString([], { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });
        const act = el("td");
        if (restoring === b.name) {
          act.append(el("span", { textContent: "Restore? Your profiles now are backed up first, then Minniondeck restarts. " }),
            btn("Restore", "btn sm go", async () => {
              try { await flush(); await backend.run("restore_backup", { name: b.name }); }
              catch (e) { restoring = null; say(`Could not restore: ${e}`); renderSettings(); }
            }),
            btn("Keep", "btn sm", () => { restoring = null; renderSettings(); }));
        } else {
          act.append(btn("Restore…", "btn sm", () => { restoring = b.name; renderSettings(); }));
        }
        tr.append(el("td", { textContent: when }), el("td", { textContent: `${Math.max(1, Math.round(b.bytes / 1024))} KB` }), act);
        tb.appendChild(tr);
      });
      t.appendChild(tb);
      w.appendChild(t);
    });
    return box;
  }

  // ----- Start -----
  // Load saved data, then show the app and start saving. Files opened by double-click before
  // the app was running are imported last.
  // Installed plugins (in <app data>\plugins\): their actions join the catalogue before saved buttons
  // are read. A plugin whose files are broken shows as Stopped, with the reason in Settings.
  async function loadPlugins() {
    applyPluginList((await backend.call("list_plugins", {}, [])) || []);
  }
  // Installed plugins, whether each is running, and their actions. Runs at start and whenever a
  // plugin starts, stops or is reloaded (plugins run in a helper process; see plugins.rs).
  function applyPluginList(list) {
    plugins.length = 0;
    clearPluginMedia();
    list.forEach((p) => {
      setPluginMedia(p);
      plugins.push({ name: p.name, id: p.id, version: p.version, state: p.state || "ready", error: p.error || "", signature: p.signature || null, hasSettings: !!p.hasSettings });
      if (!PLUGIN_BODY[p.name]) PLUGIN_BODY[p.name] = "lilac";
      (p.actions || []).forEach((a) => addPluginAction(p, a));
    });
  }
  function listenToPlugins() {
    backend.listen("plugins_changed", async () => {
      applyPluginList((await backend.call("list_plugins", {}, [])) || []);
      Object.keys(DYN_OPTS).forEach((k) => delete DYN_OPTS[k]);
      // Plugins that (re)started hear about the buttons on screen again
      resetShown();
      crewChanged();
      if (A.view === "settings") renderSettings();
      if (A.view === "deck") { renderMain(); renderInsp(); }
    });
    // A plugin changed a button: its state (saved, like a press) or its text or icon (live only)
    backend.listen("plugin_button", (e) => {
      const t = tileById(e.button);
      if (!t) return;
      if ("state" in e) {
        const n = Math.max(0, Math.min(t.states.length - 1, Number(e.state) || 0));
        // Not saved: the backend sends phones just this state (Outgoing::State)
        if (!("_savedCur" in t)) t._savedCur = t.cur || 0;
        t.cur = n;
      }
      if ("title" in e || "icon" in e) {
        const live = LIVE[t.id] || (LIVE[t.id] = {});
        if ("title" in e) live.title = e.title;
        if ("icon" in e) live.icon = e.icon;
        if (live.title == null && !live.icon) delete LIVE[t.id];
      }
      // Live data a button draws itself (a map): the button is drawn again only when it starts or
      // stops being a map; otherwise just the map is, so explore mode and animations carry on
      if ("data" in e) {
        const was = screenDataOf(t) && screenDataOf(t).kind;
        mergeLiveData(t.id, e.data);
        if (was !== (screenDataOf(t) && screenDataOf(t).kind) && window.syncTileEls) window.syncTileEls(t, null);
        drawMaps(t.id);
        return;
      }
      if (window.syncTileEls) window.syncTileEls(t, null);
    });
    backend.listen("plugin_toast", (e) => {
      const p = plugins.find((x) => x.id === e.plugin);
      say(`${p ? p.name : e.plugin}: ${e.text}`);
    });
    backend.listen("plugin_options_changed", (e) => {
      const pre = `plug:${e.plugin}:${e.action || ""}`;
      Object.keys(DYN_OPTS).filter((k) => k.startsWith(pre)).forEach((k) => delete DYN_OPTS[k]);
      if (draft && draft.steps[sel] && draft.steps[sel].type.startsWith(pre)) renderFields();
    });
  }
  // A saved button can use a plugin that isn't installed (any more): it keeps its steps and says
  // the plugin is not installed, instead of breaking the editor. A step whose action no longer
  // exists at all (the old example OBS, Spotify, Twitch and Discord actions) becomes an empty
  // "Press keys" step: the button keeps its look and asks for an action in the editor.
  function keepMissingPluginActions() {
    let replaced = 0;
    Object.values(boards).forEach((tiles) => (tiles || []).forEach((t) => (t.steps || []).forEach((s, i) => {
      if (ACTION[s.type]) return;
      if (!String(s.type).startsWith("plug:")) { t.steps[i] = { type: "key", f: { keys: "" } }; replaced++; return; }
      const [, pid = "plugin", aid = "action"] = s.type.split(":");
      if (!PLUGIN_BODY[pid]) PLUGIN_BODY[pid] = "lilac";
      addPluginAction({ id: pid, name: pid }, { id: aid, label: aid });
    })));
    if (replaced) say(`${replaced} ${replaced === 1 ? "step used an action" : "steps used actions"} that no longer exist. Those buttons now need an action: open them in the editor.`);
  }

  async function startApp() {
    let loaded = { warnings: [], ok: true };
    await loadPlugins();
    try { loaded = await loadSaved(); } catch (e) { console.error("[save] load failed:", e); logError("load", e); loaded = { warnings: ["Saved data could not be loaded."], ok: false }; }
    // Auto-save starts as soon as the saved data is in, before anything is drawn: it used to start
    // last, so an error drawing the first page left it off for the whole run
    if (loaded.ok && backend.available()) { SAVE.on = true; setInterval(tick, TICK_MS); }
    uiLog(SAVE.on ? "auto-save on" : `auto-save off (loaded: ${loaded.ok}, backend: ${backend.available()})`);
    keepMissingPluginActions();
    listenToPlugins();
    // Paired phones, kept up to date as they connect, leave or are changed
    setPhones(await backend.call("get_phones", {}, []));
    backend.listen("phones_changed", (list) => {
      setPhones(list);
      renderRail();
      if (A.view === "phones") renderPhones(true);
      if (A.view === "deck") renderInsp();
    });
    backend.listen("pairing_changed", onPairingChanged);
    Object.assign(APP, (await backend.call("app_info", {}, null)) || {});
    applyAppearance();
    go("deck");
    // "Start minimised to the tray" leaves the window hidden: click the minnion to open it
    // Started at sign-in (Start with Windows) also stays in the tray
    if (!SET.tray && !(await backend.call("started_in_tray", {}, false))) backend.showWindow();
    if (loaded.warnings.length) say(loaded.warnings[0]);
    await loadPluginConfig();
    for (const path of (await backend.call("take_pending_open", {}, [])) || []) await openFile(path);
  }
