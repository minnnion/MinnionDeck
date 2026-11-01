  // ===== Updates =====
  // Minniondeck itself updates from the public repository's GitHub Releases (app_update.rs): check,
  // download the installer (checked against GitHub's SHA-256), then Install now starts it and closes
  // the app. Plugins update from the Marketplace page. Both are checked once a day when the
  // switches in Settings are on; what's waiting shows as a badge on the rail.
  const UPD = { step: "", info: null, error: "", got: 0, total: 0, run: 0 };
  const UPD_EVERY = 20 * 3600 * 1000;

  const appUpdateWaiting = () => !!(UPD.info && UPD.info.newer);
  // Installed plugins the marketplace has a newer version of
  function pluginUpdates() {
    if (!MK.data) return [];
    return MK.data.plugins.filter((p) => { const have = mkInstalled(p); return have && verCmp(p.version, have.version || "0") > 0; });
  }

  async function appUpdateCheck(quiet) {
    if (!backend.available() || UPD.step === "checking" || UPD.step === "get") return;
    const run = ++UPD.run;
    UPD.step = "checking";
    UPD.error = "";
    renderUpdateBox();
    try {
      UPD.info = await backend.run("app_update_check");
      if (run !== UPD.run) return;
      UPD.step = "ok";
      if (quiet && UPD.info.newer) say(`Minniondeck ${UPD.info.latest} is out. Get it in Settings › Updates.`);
    } catch (e) {
      if (run !== UPD.run) return;
      UPD.step = quiet ? "" : "err";
      UPD.error = String(e);
    }
    renderUpdateBox();
    renderRail();
  }

  async function appUpdateDownload() {
    const run = ++UPD.run;
    Object.assign(UPD, { step: "get", got: 0, total: UPD.info ? UPD.info.size : 0, error: "" });
    renderUpdateBox();
    try {
      await backend.run("app_update_download");
      if (run !== UPD.run) return;
      UPD.step = "ready";
    } catch (e) {
      if (run !== UPD.run) return;
      UPD.step = "err";
      UPD.error = String(e);
    }
    renderUpdateBox();
  }
  function appUpdateCancel() {
    backend.call("app_update_cancel");
    UPD.run++;
    UPD.step = "ok";
    renderUpdateBox();
  }
  async function appUpdateInstall() {
    try { await backend.run("app_update_install"); }
    catch (e) { UPD.step = "err"; UPD.error = String(e); renderUpdateBox(); }
  }

  backend.listen("app_update_progress", (e) => {
    if (UPD.step !== "get") return;
    UPD.got = e.got;
    UPD.total = e.total || UPD.total;
    const pct = UPD.total ? Math.min(100, Math.round((UPD.got / UPD.total) * 100)) : 0;
    const bar = $("upd-fill"), words = $("upd-got");
    if (bar) bar.style.width = `${pct}%`;
    if (words) words.textContent = `${mkSize(UPD.got)} of ${mkSize(UPD.total)} · ${pct}%`;
  });

  // The box in Settings › Updates; it redraws itself in place
  function appUpdateBox() {
    const box = el("div", { className: "stack upd-box", id: "upd-box" });
    fillUpdateBox(box);
    return box;
  }
  function renderUpdateBox() {
    const box = $("upd-box");
    if (box) fillUpdateBox(box);
  }
  function fillUpdateBox(box) {
    box.textContent = "";
    const ver = APP.version || "";
    const m = el("span");
    const i = UPD.info;
    m.innerHTML = minnion({ c: pal("mint"), expr: appUpdateWaiting() ? "happy" : "open", scale: 3 });
    let words = `Minniondeck${ver ? ` ${ver}` : ""} for Windows.`;
    if (!backend.available()) words += " Updates are checked in the Minniondeck desktop app.";
    else if (UPD.step === "checking") words += " Checking GitHub for a newer version…";
    else if (i && !i.newer) words += ` You have the latest version (${i.latest}).`;
    else if (i && i.newer) words += ` Version ${i.latest} is out.`;
    else words += " Press Check for updates to look on GitHub.";
    box.append(el("div", { className: "row" }, [m, el("p", { textContent: words })]));
    const row = el("div", { className: "row" });
    const check = btn("Check for updates", "btn", () => appUpdateCheck(false));
    check.disabled = !backend.available() || UPD.step === "checking" || UPD.step === "get";
    if (UPD.step === "get") {
      const pct = UPD.total ? Math.min(100, Math.round((UPD.got / UPD.total) * 100)) : 0;
      const fill = el("i", { id: "upd-fill" });
      fill.style.width = `${pct}%`;
      const bar = el("div", { className: "mi-bar" }, [fill]);
      bar.setAttribute("role", "progressbar");
      bar.setAttribute("aria-label", "Download");
      box.append(el("p", { className: "mi-status", textContent: `Downloading ${i ? i.name : "the update"} from GitHub` }), bar, el("p", { className: "muted num", id: "upd-got", textContent: `${mkSize(UPD.got)} of ${mkSize(UPD.total)} · ${pct}%` }));
      row.append(btn("Cancel", "btn", appUpdateCancel));
    } else if (UPD.step === "ready") {
      box.append(el("p", { className: "mi-status ok", textContent: "Downloaded, and it matches GitHub's checksum." }), el("p", { className: "help", textContent: "Install now closes Minniondeck and starts the installer. Your profiles, buttons and plugins stay." }));
      row.append(btn("Install now", "btn go", appUpdateInstall), btn("Later", "btn", () => { UPD.step = "ok"; renderUpdateBox(); }));
    } else if (i && i.newer) {
      row.append(btn(`Download ${i.latest}${i.size ? ` (${mkSize(i.size)})` : ""}`, "btn go", appUpdateDownload), check);
    } else row.append(check);
    if (UPD.step === "err") box.append(el("p", { className: "warn", textContent: UPD.error }));
    box.append(row);
    if (i && i.newer && i.notes && UPD.step !== "get") {
      const notes = el("details", { className: "upd-notes" }, [el("summary", { textContent: `What's new in ${i.latest}` }), el("pre", { textContent: i.notes })]);
      box.append(notes);
    }
  }

  // Once a day, while the switches are on: Minniondeck from GitHub Releases, plugins from the marketplace
  async function dailyUpdateCheck() {
    if (!backend.available() || Date.now() - (SET.updChecked || 0) < UPD_EVERY) return;
    if (!SET.appUpd && !SET.plugUpd) return;
    SET.updChecked = Date.now();
    backend.call("save_settings", { settings: SET });
    if (SET.appUpd) appUpdateCheck(true);
    if (SET.plugUpd) {
      await marketLoad(true);
      const n = pluginUpdates().length;
      if (n) say(`${n} ${n === 1 ? "plugin has an update" : "plugins have updates"} in the Marketplace.`);
      renderRail();
    }
  }
  function startUpdateChecks() {
    setTimeout(dailyUpdateCheck, 8000);
    setInterval(dailyUpdateCheck, 3600 * 1000);
  }
