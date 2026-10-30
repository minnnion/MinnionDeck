  // ===== Settings =====
  function switchEl(on, label, fn) {
    const b = el("button", { type: "button", className: "switch" });
    b.setAttribute("role", "switch");
    b.setAttribute("aria-checked", String(on));
    b.setAttribute("aria-label", label);
    b.addEventListener("click", () => { const v = b.getAttribute("aria-checked") !== "true"; b.setAttribute("aria-checked", String(v)); fn(v); });
    return b;
  }
  // Start with Windows adds or removes the sign-in entry (src-tauri/src/autostart.rs).
  // If Windows refuses, the switch goes back so it never shows on when nothing is set up.
  async function setStartWithWindows(on) {
    if (!backend.available()) return;
    try {
      await backend.run("set_start_with_windows", { enabled: on });
      say(on ? "Minniondeck will start in the tray when you sign in" : "Minniondeck won't start when you sign in");
    } catch (e) {
      SET.startWin = !on;
      say(`${e}`);
      if (A.view === "settings") renderSettings();
    }
  }
  // Settings > Phones > Port: a whole number from 1024 to 65535, used straight away.
  // Returns the error to show next to the field, or "" when it worked.
  async function applyPort(text) {
    if (!/^\d+$/.test(text)) return "Enter a whole number, like 47800.";
    const port = Number(text);
    if (port < 1024 || port > 65535) return "Choose a port from 1024 to 65535.";
    if (String(port) === String(SET.port)) return "";
    if (!backend.available()) { SET.port = String(port); return ""; }
    try {
      await backend.run("set_phone_port", { port });
      SET.port = String(port);
      say(`Phones now connect on port ${port}`);
      return "";
    } catch (e) { return String(e); }
  }
  const themeValue = () => document.documentElement.dataset.theme || "system";
  const sw = (key, label, help) => ({ kind: "switch", key, label, help });
  const sg = (key, label, opts, help) => ({ kind: "seg", key, label, opts, help });
  const sl = (key, label, opts, help) => ({ kind: "select", key, label, opts, help });
  const tx = (key, label, help) => ({ kind: "text", key, label, help });
  const cu = (render) => ({ kind: "custom", render });
  // One of the app's own folders, shown as it is (read-only) with a button that opens it
  const fd = (which, label, help) => ({ kind: "folder", which, label, help });
  // The real version and folders, from the backend (applog.rs). Filled in when the app starts.
  const APP = { version: "", dataDir: "", backupsDir: "", logsDir: "", pluginsDir: "" };
  const openFolder = (which) => backend.run("open_app_folder", { which }).catch((e) => say(String(e)));
  // A row that isn't ready yet: shown greyed out with a "Coming soon" tag, and can't be changed
  const soon = (row) => ({ ...row, soon: true });
  const soonTag = () => el("span", { className: "soon-tag", textContent: "Coming soon" });

  // Appearance takes effect at once: theme, motion, button size and names on the Deck.
  // Phones get Animations and Show button names through the saved settings.
  function applyAppearance() {
    if (SET.theme === "light" || SET.theme === "dark") document.documentElement.dataset.theme = SET.theme;
    else delete document.documentElement.dataset.theme;
    try { if (SET.theme === "light" || SET.theme === "dark") localStorage.setItem("minniondeck-theme", SET.theme); else localStorage.removeItem("minniondeck-theme"); } catch (e) {}
    syncTheme();
    setMotion(SET.anim);
    const w = $("appwin");
    w.dataset.tile = SET.tileSize;
    w.classList.toggle("no-names", !SET.names);
  }
  const TABS = [
    ["general", "General", "gear", () => [
      sw("startWin", "Start with Windows", "Minniondeck starts in the tray when you sign in, so your buttons and phones are ready."),
      sw("tray", "Start minimised to the tray"),
      sg("close", "When you close the window", [["tray", "Keep running"], ["quit", "Quit"]], "Keep running so your buttons and phones still work."),
      // The other languages are kept for later; they can't be picked until they are translated
      sl("lang", "Language", [["English", "English"], ["हिन्दी", "हिन्दी · Coming soon", true], ["Deutsch", "Deutsch · Coming soon", true], ["Español", "Español · Coming soon", true], ["日本語", "日本語 · Coming soon", true]], "More languages are on the way. For now Minniondeck is in English.")
    ]],
    ["appearance", "Appearance", "sun", () => [
      { kind: "theme" },
      sg("anim", "Animations", [["full", "Full"], ["reduced", "Reduced"], ["off", "Off"]], "Reduced: the crew stops walking, small effects stay. Off: nothing moves."),
      soon(sw("crew", "Show the crew on the board", "The crew isn't on the Deck yet.")),
      sg("tileSize", "Button size", [["s", "Small"], ["m", "Medium"], ["l", "Large"]], "How much of the Deck the buttons fill."),
      sw("names", "Show button names", "Also hides names on phones.")
    ]],
    ["profiles", "Profiles and pages", "people", () => [
      sw("autoSwitch", "Switch profiles automatically", "Switches when the app chosen on a profile comes to the front. Using Minniondeck itself never switches."),
      cu(rulesTable),
      sw("backBtn", "Show a Back button inside folders", "Pages inside a folder get a Back button in their first free spot, on the Deck and on phones."),
      sl("startPage", "Open on start", [["last", "Where I left off"], ...PROFILES.flatMap((q) => pagesOf(q).map(({ page, path }) => [page.id, [q.name, ...path.map((f) => f.name), page.name].join(" / ")]))])
    ]],
    ["phones", "Phones", "phone", () => [
      sg("conn", "Connect over", [["wifi", "Wi-Fi"], ["usb", "USB", true], ["both", "Both", true]], "Phones connect over Wi-Fi. USB and Both are coming soon."),
      { ...tx("port", "Port", "Change this only if another app uses it. Connected phones move with it; phones that are off will need pairing again."), apply: applyPort },
      sw("pin", "Show a matching number when a new phone pairs"),
      sw("followPc", "Phones follow the page open on this PC"),
      sw("awake", "Keep phone screens awake", "Applies on phones."),
      sw("haptics", "Vibrate when a button is pressed", "Applies on phones."),
      cu(() => el("div", { className: "row" }, [btn("Manage phones and updates", "btn", () => go("phones"))]))
    ]],
    ["plugins", "Plugins", "bolt", () => [
      ...(PLUG.offer ? [cu(pluginOffer)] : []),
      cu(pluginTable),
      { ...sw("devMode", "Developer mode", "Runs plugins that aren't signed by a trusted author, like ones you are building. Turn it on only for plugins you trust."), get: () => PLUG.cfg.devMode, apply: setDevMode },
      cu(authorsPanel),
      fd("plugins", "Plugins folder", "Each plugin has its own folder here. Install plugins with Install a plugin… or by opening a .mdplugin file."),
      soon(sw("plugUpd", "Check plugins for updates every day"))
    ]],
    ["simbrief", "SimBrief", "note", () => [cu(simbriefPanel)]],
    ["backup", "Backup", "folder", () => [
      sw("autoBackup", "Back up when Minniondeck starts", "Only when something changed since the last backup. A backup is also made before every import and restore."),
      sl("keep", "Backups to keep", ["1", "3", "5"]),
      fd("backups", "Backup folder"),
      cu(() => el("div", { className: "row" }, [btn("Export all profiles…", "btn", exportAllProfiles), btn("Import…", "btn", pickAndImport)])),
      cu(backupPanel)
    ]],
    ["updates", "Updates", "down", () => [
      cu(desktopUpdate),
      soon(sg("appChannel", "Update channel", [["stable", "Stable"], ["beta", "Beta"]])),
      soon(sw("bundlePhone", "Offer phone app updates when the desktop app updates")),
      cu(() => { const b = btn("Push the phone app now", "btn", null); b.disabled = true; return el("div", { className: "row" }, [b, soonTag()]); })
    ]],
    ["about", "About", "star", () => [cu(about)]]
  ];
  function renderSettings() {
    const box = $("av-settings");
    const focusedTab = document.activeElement && document.activeElement.dataset && document.activeElement.dataset.stab;
    box.innerHTML = "";
    const wrap = el("div", { className: "settings" });
    const nav = el("div", { className: "snav" });
    nav.setAttribute("role", "tablist");
    nav.setAttribute("aria-label", "Settings");
    nav.setAttribute("aria-orientation", "vertical");
    TABS.forEach(([id, label, icon]) => {
      const b = el("button", { type: "button", id: `st-${id}` });
      b.dataset.stab = id;
      b.setAttribute("role", "tab");
      b.setAttribute("aria-selected", String(settingsTab === id));
      b.setAttribute("aria-controls", "a-sbody");
      b.tabIndex = settingsTab === id ? 0 : -1;
      b.innerHTML = `${glyph(icon, 18)}<span></span>`;
      b.lastChild.textContent = label;
      b.addEventListener("click", () => { settingsTab = id; renderSettings(); $(`st-${id}`).focus(); });
      nav.appendChild(b);
    });
    nav.addEventListener("keydown", (e) => {
      const keys = { ArrowDown: 1, ArrowRight: 1, ArrowUp: -1, ArrowLeft: -1 };
      if (!(e.key in keys)) return;
      e.preventDefault();
      const i = TABS.findIndex(([id]) => id === settingsTab);
      settingsTab = TABS[(i + keys[e.key] + TABS.length) % TABS.length][0];
      renderSettings();
      $(`st-${settingsTab}`).focus();
    });
    const tab = TABS.find(([id]) => id === settingsTab);
    const body = el("div", { className: "sbody", id: "a-sbody" });
    body.setAttribute("role", "tabpanel");
    body.setAttribute("aria-labelledby", `st-${settingsTab}`);
    body.appendChild(el("h3", { textContent: tab[1] }));
    tab[3]().forEach((c) => body.appendChild(settingRow(c)));
    wrap.append(nav, body);
    box.appendChild(wrap);
    if (focusedTab && $(`st-${focusedTab}`)) $(`st-${focusedTab}`).focus();
  }
  function settingRow(c) {
    if (c.kind === "custom") { const r = el("div", { className: "srow wide" }); r.appendChild(c.render()); return r; }
    const r = el("div", { className: "srow" });
    const lab = el("div", { className: "lab" });
    const id = `set-${c.key || c.which || c.kind}`;
    const title = el("b", { id: `${id}-l`, textContent: c.kind === "theme" ? "Theme" : c.label });
    lab.appendChild(title);
    if (c.soon) { title.append(" ", soonTag()); r.classList.add("soon"); }
    const helpText = c.kind === "theme" ? "System follows your Windows setting. Same as the Dark mode button at the top of this page." : c.help;
    if (helpText) lab.appendChild(el("span", { textContent: helpText }));
    let ctrl;
    if (c.kind === "switch") ctrl = switchEl(c.soon ? false : c.get ? c.get() : SET[c.key], c.label, (v) => {
      // A switch that isn't a saved setting does its own thing
      if (c.apply) { c.apply(v); return; }
      SET[c.key] = v;
      applyAppearance();
      if (c.key === "autoSwitch") renderStatus();
      if (c.key === "startWin") setStartWithWindows(v);
    });
    if (c.kind === "seg" || c.kind === "theme") {
      ctrl = el("div", { className: "seg" });
      ctrl.setAttribute("role", "group");
      ctrl.setAttribute("aria-labelledby", `${id}-l`);
      const opts = c.kind === "theme" ? [["system", "System"], ["light", "Light"], ["dark", "Dark"]] : c.opts;
      const val = c.kind === "theme" ? themeValue() : SET[c.key];
      opts.forEach(([v, l, off]) => {
        const b = btn(l, "", () => {
          if (c.kind === "theme") {
            if (v === "system") { delete document.documentElement.dataset.theme; try { localStorage.removeItem("minniondeck-theme"); } catch (e) {} }
            else { document.documentElement.dataset.theme = v; try { localStorage.setItem("minniondeck-theme", v); } catch (e) {} }
            SET.theme = v;
            applyAppearance();
          } else { SET[c.key] = v; applyAppearance(); }
          ctrl.querySelectorAll("button").forEach((x) => x.setAttribute("aria-pressed", String(x === b)));
        });
        b.setAttribute("aria-pressed", String(v === val));
        if (c.soon || off) b.disabled = true;
        ctrl.appendChild(b);
      });
    }
    if (c.kind === "select") {
      ctrl = el("select", { id });
      ctrl.setAttribute("aria-labelledby", `${id}-l`);
      // An option can be listed but not chosen yet ([value, label, true])
      c.opts.forEach((o) => { const [v, l, off] = Array.isArray(o) ? o : [o, o]; ctrl.appendChild(el("option", { value: v, textContent: l, disabled: !!off })); });
      const usable = [...ctrl.options].filter((o) => !o.disabled);
      if (usable.length && !usable.some((o) => o.value === SET[c.key])) SET[c.key] = usable[0].value;
      ctrl.value = SET[c.key];
      ctrl.addEventListener("change", () => { SET[c.key] = ctrl.value; });
    }
    if (c.kind === "text") {
      ctrl = el("input", { type: "text", id, value: SET[c.key], inputMode: "numeric" });
      ctrl.setAttribute("aria-labelledby", `${id}-l`);
      // Any problem is named next to the field it belongs to
      const err = el("span", { className: "err", id: `${id}-err` });
      err.setAttribute("role", "alert");
      ctrl.setAttribute("aria-describedby", `${id}-err`);
      lab.appendChild(err);
      if (c.apply) {
        ctrl.addEventListener("change", async () => {
          const problem = await c.apply(ctrl.value.trim());
          err.textContent = problem || "";
          ctrl.setAttribute("aria-invalid", String(!!problem));
        });
      } else ctrl.addEventListener("input", () => { SET[c.key] = ctrl.value; });
    }
    if (c.kind === "folder") {
      r.classList.add("wide");
      ctrl = el("div", { className: "pathbox" });
      const shown = el("input", { type: "text", id, value: APP[`${c.which}Dir`] || "Shown in the desktop app", readOnly: true });
      shown.setAttribute("aria-labelledby", `${id}-l`);
      const open = btn("Open folder", "btn sm", () => openFolder(c.which));
      open.disabled = !backend.available();
      ctrl.append(shown, open);
    }
    if (c.soon && ctrl) ctrl.disabled = true;
    r.append(lab, ctrl);
    return r;
  }
  function rulesTable() {
    const w = el("div", { className: "table-wrap" });
    const t = el("table");
    t.innerHTML = "<thead><tr><th>When this app is in front</th><th>Switch to</th></tr></thead>";
    const tb = el("tbody");
    PROFILES.filter((p) => p.auto && p.auto !== "Never").forEach((p) => { const tr = el("tr"); tr.append(el("td", { textContent: p.auto }), el("td", { textContent: p.name })); tb.appendChild(tr); });
    if (!tb.children.length) { const tr = el("tr"); tr.append(el("td", { textContent: "No rules yet. Choose an app on a profile in Profiles." })); tr.firstChild.colSpan = 2; tb.appendChild(tr); }
    t.appendChild(tb);
    w.appendChild(t);
    return el("div", { className: "stack" }, [w, el("div", { className: "row" }, [btn("Edit rules in Profiles", "btn sm", () => go("profiles"))])]);
  }
  // ----- Settings > SimBrief -----
  // The MSFS 2024 plugin (minnioncore_deck) fetches the pilot's latest SimBrief flight plan (OFP)
  // for its OFP screens and the map. These are that plugin's own settings (its manifest's
  // simbrief_user and simbrief_route), kept here where they're easy to find.
  const SB = { loaded: false, user: "", route: true, msg: "", ok: true, missing: false, busy: false };
  const SB_PLUGIN = "minnioncore_deck";
  async function loadSimbrief() {
    if (!backend.available()) { SB.loaded = true; SB.missing = true; return; }
    try {
      const got = await backend.run("plugin_settings", { id: SB_PLUGIN });
      SB.user = (got.values && got.values.simbrief_user) || "";
      SB.route = !(got.values && got.values.simbrief_route === false);
      SB.missing = false;
    } catch (e) { SB.missing = true; }
    SB.loaded = true;
    if (A.view === "settings" && settingsTab === "simbrief") renderSettings();
  }
  function simbriefPanel() {
    if (!SB.loaded) { loadSimbrief(); return el("p", { className: "help", textContent: "Loading…" }); }
    if (SB.missing) return el("p", { className: "help", textContent: "SimBrief comes with the MSFS 2024 plugin. Install it in Plugins, then come back here." });
    const box = el("div", { className: "stack sb-panel" });
    box.append(el("p", { className: "help", textContent: "Your latest SimBrief flight plan (OFP) shows on OFP buttons (on the Taxi Map page), to scroll, on this PC and on phones, and its route is drawn on the map. SimBrief gives anyone's latest plan by name, so no password is needed." }));
    const user = el("input", { type: "text", id: "sb-user", value: SB.user, placeholder: "Username, or Pilot ID (a number)", autocomplete: "off", spellcheck: false });
    user.addEventListener("input", () => { SB.user = user.value; });
    const userRow = el("div", { className: "field" }, [el("label", { htmlFor: "sb-user", textContent: "SimBrief username or Pilot ID" }), user,
      el("span", { className: "help", textContent: "Your Pilot ID is on SimBrief's Account page (Account > Pilot ID)." })]);
    const route = switchEl(SB.route, "Draw the SimBrief route on maps", (v) => { SB.route = v; });
    const routeRow = el("div", { className: "row" }, [route, el("span", { textContent: "Draw the SimBrief route on maps (dashed)" })]);
    const out = el("p", { className: SB.ok ? "help" : "err", textContent: SB.msg });
    out.setAttribute("role", "status");
    const save = btn("Save and fetch my OFP", "btn go", async () => {
      SB.busy = true; save.disabled = true;
      try {
        await backend.run("save_plugin_settings", { id: SB_PLUGIN, values: { simbrief_user: SB.user.trim(), simbrief_route: SB.route } });
        SB.msg = SB.user.trim() ? await backend.run("plugin_settings_button", { id: SB_PLUGIN, key: "simbrief_fetch" }) : "Saved. Type your username to get your OFP.";
        SB.ok = true;
      } catch (e) { SB.msg = String(e); SB.ok = false; }
      SB.busy = false;
      renderSettings();
    });
    const fetchNow = btn("Fetch again", "btn", async () => {
      try { SB.msg = await backend.run("plugin_settings_button", { id: SB_PLUGIN, key: "simbrief_fetch" }); SB.ok = true; } catch (e) { SB.msg = String(e); SB.ok = false; }
      renderSettings();
    });
    save.disabled = SB.busy;
    box.append(userRow, routeRow, el("div", { className: "row" }, [save, fetchNow]), out);
    return box;
  }

  // ----- Settings > Plugins -----
  // Developer mode and trusted authors come from the backend (plugins.json, see plugins.rs).
  // A plugin offered for install (double-clicked .mdplugin, or Install a plugin…) waits in
  // PLUG.offer until the user says Install or Cancel.
  const PLUG = { cfg: { devMode: false, authors: [], myKey: null }, offer: null, removing: null, busy: false, form: null };
  // A plugin's own settings, from the "settings" list in its manifest.json (see
  // plugin_settings.rs). Passwords are never sent here: the form only knows if one is saved,
  // and an empty password box keeps it. Typing doesn't redraw the page, so focus stays put.
  async function openPluginSettingsForm(p) {
    try {
      const got = await backend.run("plugin_settings", { id: p.id });
      PLUG.form = { id: p.id, name: p.name, fields: got.fields || [], values: got.values || {}, draft: {}, error: "", results: {} };
      renderSettings();
      const first = document.querySelector(".plug-form input, .plug-form select, .plug-form .switch");
      if (first) first.focus();
    } catch (e) { say(`${e}`); }
  }
  function pluginForm() {
    const f = PLUG.form;
    const box = el("div", { className: "plug-form" });
    box.setAttribute("role", "group");
    box.setAttribute("aria-label", `${f.name} settings`);
    box.append(el("b", { textContent: `${f.name} settings` }));
    const val = (k) => (k in f.draft ? f.draft[k] : f.values[k]);
    f.fields.forEach((fd) => {
      const id = `ps-${f.id}-${fd.key}`;
      const row = el("div", { className: "field" });
      if (fd.kind === "button") {
        const res = f.results[fd.key];
        const out = el("span", { className: res ? (res.ok ? "help" : "err") : "help", textContent: res ? res.text : (fd.help || "") });
        out.setAttribute("role", "status");
        const b = btn(fd.label, "btn sm", async () => {
          b.disabled = true;
          out.className = "help";
          out.textContent = "Working…";
          // What was typed is saved first, so the button uses it (a Client ID pasted just now)
          try {
            if (Object.keys(f.draft).length) {
              await backend.run("save_plugin_settings", { id: f.id, values: f.draft });
              f.values = (await backend.run("plugin_settings", { id: f.id })).values || f.values;
              f.draft = {};
            }
            f.results[fd.key] = { ok: true, text: await backend.run("plugin_settings_button", { id: f.id, key: fd.key }) };
          }
          catch (e) { f.results[fd.key] = { ok: false, text: String(e) }; }
          renderSettings();
        });
        row.append(el("div", { className: "row" }, [b]), out);
        box.append(row);
        return;
      }
      const label = el("label", { htmlFor: id, textContent: fd.required ? fd.label : `${fd.label} (optional)` });
      let ctrl;
      if (fd.kind === "switch") {
        ctrl = switchEl(!!val(fd.key), fd.label, (v) => { f.draft[fd.key] = v; });
        ctrl.id = id;
      } else if (fd.kind === "select") {
        ctrl = el("select", { id });
        fd.options.forEach((o) => ctrl.appendChild(el("option", { value: o, textContent: o })));
        ctrl.value = val(fd.key) ?? fd.options[0];
        ctrl.addEventListener("change", () => { f.draft[fd.key] = ctrl.value; });
      } else if (fd.kind === "password") {
        const set = f.values[fd.key] && f.values[fd.key].set && f.draft[fd.key] !== null;
        ctrl = el("input", { id, type: "password", autocomplete: "off", placeholder: set ? "Saved. Type to replace it" : fd.placeholder || "" });
        ctrl.addEventListener("input", () => { f.draft[fd.key] = ctrl.value; });
        if (set) {
          const clear = btn("Clear", "btn sm", () => { f.draft[fd.key] = null; renderSettings(); });
          clear.setAttribute("aria-label", `Clear the saved ${fd.label}`);
          ctrl = el("div", { className: "row" }, [ctrl, clear]);
        } else if (f.draft[fd.key] === null) {
          row.append(label, ctrl, el("span", { className: "help", textContent: "Cleared when you save." }));
          box.append(row);
          return;
        }
      } else {
        ctrl = el("input", { id, type: fd.kind === "number" ? "number" : "text", placeholder: fd.placeholder || "" });
        if (fd.kind === "number") { if (fd.min != null) ctrl.min = fd.min; if (fd.max != null) ctrl.max = fd.max; ctrl.inputMode = "decimal"; }
        const v = val(fd.key);
        ctrl.value = v == null ? "" : String(v);
        ctrl.addEventListener("input", () => { f.draft[fd.key] = ctrl.value; });
      }
      row.append(label, ctrl);
      if (fd.help) row.append(el("span", { className: "help", textContent: fd.help }));
      box.append(row);
    });
    const err = el("p", { className: "err", textContent: f.error });
    err.setAttribute("role", "alert");
    const save = btn("Save", "btn go", async () => {
      save.disabled = true;
      try {
        const msg = await backend.run("save_plugin_settings", { id: f.id, values: f.draft });
        PLUG.form = null;
        say(`${f.name}: ${msg}`);
      } catch (e) { f.error = String(e); }
      renderSettings();
    });
    box.append(err, el("div", { className: "row" }, [save, btn("Cancel", "btn", () => { PLUG.form = null; renderSettings(); })]));
    return box;
  }

  async function loadPluginConfig() {
    PLUG.cfg = (await backend.call("plugin_config", {}, null)) || PLUG.cfg;
  }
  // Run a plugin command that answers with the new plugin list, then show it
  async function pluginCommand(cmd, args, done) {
    PLUG.busy = true;
    renderSettings();
    try {
      applyPluginList(await backend.run(cmd, args));
      resetShown();
      if (done) say(done);
    } catch (e) { say(`${e}`); }
    PLUG.busy = false;
    await loadPluginConfig();
    if (A.view === "settings") renderSettings();
  }
  // Show what a .mdplugin is and ask before installing it
  async function offerPlugin(path) {
    try {
      const info = await backend.run("inspect_plugin", { path });
      PLUG.offer = { path, info };
      window.openPluginSettings();
    } catch (e) { say(`Could not open that plugin: ${e}`); }
  }
  async function pickPluginFile() {
    try {
      const path = await backend.run("pick_plugin_file", { kind: "plugin" });
      if (path) await offerPlugin(path);
    } catch (e) { say(`${e}`); }
  }
  // Who signed a plugin, in words
  function signedBy(sig) {
    if (!sig) return "Not signed";
    if (sig.trust === "trusted") return `${sig.author}, trusted`;
    if (sig.trust === "user") return `${sig.author}, trusted by you`;
    if (sig.trust === "untrusted") return `${sig.author}, not trusted`;
    if (sig.trust === "bad") return `Damaged: ${sig.why}`;
    return "Not signed";
  }
  function pluginOffer() {
    const { path, info } = PLUG.offer;
    const card = el("div", { className: "plug-offer" });
    card.setAttribute("role", "region");
    card.setAttribute("aria-label", `Install ${info.name}`);
    card.append(el("b", { textContent: `Install ${info.name} ${info.version}?` }));
    const facts = [
      `${info.actions} ${info.actions === 1 ? "action" : "actions"}`,
      `Signed by: ${signedBy(info.signature)}`,
      info.installed != null ? `Replaces version ${info.installed || "(no version)"}` : null
    ].filter(Boolean);
    card.append(el("p", { className: "muted", textContent: facts.join(" · ") }));
    card.append(el("p", { className: "help", textContent: "Plugins run with the same rights as Minniondeck. Install only plugins from people you trust." }));
    if (!info.runs && info.why) card.append(el("p", { className: "warn", textContent: `It won't run yet. ${info.why}` }));
    const bad = info.signature && info.signature.trust === "bad";
    const yes = btn("Install", "btn go", () => { PLUG.offer = null; pluginCommand("install_plugin", { path }, `Installed ${info.name}`); });
    yes.disabled = bad || PLUG.busy;
    card.append(el("div", { className: "row" }, [yes, btn("Cancel", "btn", () => { PLUG.offer = null; renderSettings(); })]));
    return card;
  }
  function pluginTable() {
    const w = el("div", { className: "table-wrap" });
    const t = el("table");
    t.innerHTML = "<thead><tr><th>Plugin</th><th>Signed by</th><th>Status</th><th><span class=\"sr-only\">Actions</span></th></tr></thead>";
    const tb = el("tbody");
    plugins.forEach((p) => {
      const tr = el("tr");
      const s = el("td");
      const st = p.state;
      s.innerHTML = `<span class="dot ${st === "ready" ? "ok" : st === "waking" ? "busy" : st === "off" ? "" : "bad"}"></span>`;
      s.append(st === "ready" ? "Running" : st === "waking" ? "Starting" : st === "off" ? "Turned off" : `Stopped${p.error ? `: ${p.error}` : ""}`);
      const acts = el("td", { className: "plug-acts" });
      if (PLUG.removing === p.id) {
        const conf = el("div", { className: "pconfirm" });
        conf.setAttribute("role", "alert");
        conf.append(el("span", { textContent: `Remove ${p.name}? Buttons that use it stay, but won't run.` }), el("div", { className: "row" }, [
          btn("Remove", "btn sm go", () => { PLUG.removing = null; pluginCommand("remove_plugin", { id: p.id }, `Removed ${p.name}`); }),
          btn("Keep", "btn sm", () => { PLUG.removing = null; renderSettings(); })
        ]));
        acts.append(conf);
      } else {
        const on = st !== "off";
        const toggle = btn(on ? "Turn off" : "Turn on", "btn sm", () => pluginCommand("set_plugin_enabled", { id: p.id, on: !on }, on ? `Turned off ${p.name}` : `Turned on ${p.name}`));
        const rm = btn("Remove", "btn sm", () => { PLUG.removing = p.id; renderSettings(); });
        const conf = p.hasSettings ? btn("Settings", "btn sm", () => openPluginSettingsForm(p)) : null;
        if (conf) conf.setAttribute("aria-label", `${p.name} settings`);
        toggle.disabled = rm.disabled = PLUG.busy;
        toggle.setAttribute("aria-label", `${on ? "Turn off" : "Turn on"} ${p.name}`);
        rm.setAttribute("aria-label", `Remove ${p.name}`);
        acts.append(el("div", { className: "row" }, [conf, toggle, rm].filter(Boolean)));
      }
      tr.append(el("td", { textContent: p.version ? `${p.name} ${p.version}` : p.name }), el("td", { textContent: signedBy(p.signature) }), s, acts);
      tb.appendChild(tr);
    });
    if (!plugins.length) { const tr = el("tr"); tr.append(el("td", { textContent: "No plugins installed yet." })); tr.firstChild.colSpan = 4; tb.appendChild(tr); }
    t.appendChild(tb);
    w.appendChild(t);
    const install = btn("Install a plugin…", "btn", pickPluginFile);
    // Loads new and rebuilt plugins without restarting Minniondeck
    const reload = btn("Reload plugins", "btn", () => pluginCommand("reload_plugins", {}, "Plugins reloaded"));
    install.disabled = reload.disabled = PLUG.busy || !backend.available();
    return el("div", { className: "stack" }, [w, ...(PLUG.form ? [pluginForm()] : []), el("div", { className: "row" }, [install, reload])]);
  }
  async function setDevMode(on) {
    if (on === PLUG.cfg.devMode) return;
    PLUG.cfg.devMode = on;
    await pluginCommand("set_dev_mode", { on }, on ? "Developer mode is on: unsigned plugins can run" : "Developer mode is off");
  }
  function authorsPanel() {
    const box = el("div", { className: "stack" });
    box.append(el("b", { textContent: "Trusted authors" }), el("span", { className: "help", textContent: "Plugins signed by these authors run without Developer mode. Add an author by the .mdkey file they send you." }));
    const w = el("div", { className: "table-wrap" });
    const t = el("table");
    t.innerHTML = "<thead><tr><th>Author</th><th>Key</th><th>Added by</th><th><span class=\"sr-only\">Actions</span></th></tr></thead>";
    const tb = el("tbody");
    PLUG.cfg.authors.forEach((a) => {
      const tr = el("tr");
      const act = el("td");
      if (!a.builtIn) {
        const rm = btn("Remove", "btn sm", async () => {
          try { PLUG.cfg = await backend.run("remove_author", { key: a.key }); say(`${a.name} is no longer trusted`); } catch (e) { say(`${e}`); }
          renderSettings();
        });
        rm.setAttribute("aria-label", `Stop trusting ${a.name}`);
        act.append(rm);
      }
      tr.append(el("td", { textContent: a.name }), el("td", { className: "mono", textContent: a.fingerprint }), el("td", { textContent: a.builtIn ? "Built in" : "You" }), act);
      tb.appendChild(tr);
    });
    if (!PLUG.cfg.authors.length) { const tr = el("tr"); tr.append(el("td", { textContent: "No trusted authors yet." })); tr.firstChild.colSpan = 4; tb.appendChild(tr); }
    t.appendChild(tb);
    w.appendChild(t);
    const add = btn("Add an author key…", "btn", async () => {
      try {
        const path = await backend.run("pick_plugin_file", { kind: "key" });
        if (!path) return;
        PLUG.cfg = await backend.run("add_author", { path });
        say("Author added. Their signed plugins can run now.");
      } catch (e) { say(`${e}`); }
      renderSettings();
    });
    add.disabled = !backend.available();
    box.append(w, el("div", { className: "row" }, [add]));
    if (PLUG.cfg.myKey) box.append(el("span", { className: "help", textContent: `This PC's author key: ${PLUG.cfg.myKey.name} (${PLUG.cfg.myKey.fingerprint}). Minniondeck.exe plugin pack signs your plugins with it.` }));
    return box;
  }
  // There is no update source yet, so nothing is checked and nothing pretends to be
  function desktopUpdate() {
    const box = el("div", { className: "stack" });
    box.style.gap = "8px";
    const m = el("span");
    m.innerHTML = minnion({ c: pal("mint"), expr: "open", scale: 3 });
    const status = el("p", { textContent: `Minniondeck ${APP.version || ""} for Windows. Updates aren't set up yet, so a newer version has to be installed by hand.` });
    const b = btn("Check for updates", "btn", null);
    b.disabled = true;
    box.append(el("div", { className: "row" }, [m, status]), el("div", { className: "row" }, [b, soonTag()]));
    return box;
  }
  function about() {
    const d = el("dl", { className: "kv" });
    [["Desktop app", APP.version || "Shown in the desktop app"], ["Phone app", latest() || "Not released yet"], ["Your data", APP.dataDir || "Shown in the desktop app"], ["Logs", APP.logsDir || "Shown in the desktop app"], ["Plugins loaded", `${plugins.length}`]].forEach(([k, v]) => d.append(el("dt", { textContent: k }), el("dd", { textContent: v })));
    const m = el("span");
    m.innerHTML = minnion({ c: pal("mint"), expr: "happy", arm: true, scale: 4 });
    const logs = btn("Open logs folder", "btn sm", () => openFolder("logs"));
    logs.disabled = !backend.available();
    const lic = btn("Licences", "btn sm", null);
    lic.disabled = true;
    return el("div", { className: "stack" }, [m, d, el("div", { className: "row" }, [logs, lic, soonTag()])]);
  }

