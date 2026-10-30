  // ----- Install from the marketplace -----
  // The website's Install button opens a minniondeck://install?id=… link (see market.rs). A popup
  // shows the plugin from the marketplace list, downloads it from GitHub with a progress bar, and
  // asks before installing it. A second link while it is open replaces it.
  const MI = { link: "", step: "", info: null, pack: null, path: "", error: "", profileError: "", got: 0, total: 0, run: 0 };
  let miModal = null, miReturn = null;

  const mb = (b) => (b >= 1048576 ? `${(b / 1048576).toFixed(1)} MB` : `${Math.max(1, Math.round(b / 1024))} KB`);
  // Plain version order: 0.27.0 is newer than 0.9.1
  const verCmp = (a, b) => {
    const pa = String(a).split(/[.-]/), pb = String(b).split(/[.-]/);
    for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
      const x = parseInt(pa[i] || "0", 10) || 0, y = parseInt(pb[i] || "0", 10) || 0;
      if (x !== y) return x < y ? -1 : 1;
    }
    return 0;
  };

  function miBuild() {
    miModal = el("div", { className: "ed-modal mi-modal", hidden: true });
    const dlg = el("div", { className: "ed-dialog mi-dialog" });
    dlg.setAttribute("role", "dialog");
    dlg.setAttribute("aria-modal", "true");
    dlg.setAttribute("aria-labelledby", "mi-h");
    const close = btn("✕", "wclose", miClose, "Close");
    dlg.append(el("div", { className: "ed-titlebar" }, [el("h2", { id: "mi-h", textContent: "Install from the marketplace" }), el("span", { className: "grow" }), close]));
    dlg.append(el("div", { className: "mi-body", id: "mi-body" }));
    dlg.addEventListener("keydown", (e) => {
      if (e.key === "Escape") { e.preventDefault(); miClose(); return; }
      if (e.key !== "Tab") return;
      const f = [...dlg.querySelectorAll("button")].filter((x) => !x.disabled && x.offsetParent !== null);
      if (!f.length) return;
      if (e.shiftKey && document.activeElement === f[0]) { e.preventDefault(); f[f.length - 1].focus(); }
      else if (!e.shiftKey && document.activeElement === f[f.length - 1]) { e.preventDefault(); f[0].focus(); }
    });
    miModal.append(dlg);
    document.getElementById("appwin").append(miModal);
  }

  function miDiscard() {
    if (MI.path) backend.call("market_discard", { path: MI.path });
    MI.path = "";
  }
  function miClose() {
    if (MI.step === "get") backend.call("market_cancel");
    if (MI.step !== "busy") miDiscard();
    MI.run++;
    MI.step = "";
    if (miModal) miModal.hidden = true;
    if (miReturn && miReturn.isConnected) miReturn.focus({ preventScroll: true });
  }

  // Look the plugin up, then download it straight away; installing waits for the user
  async function openInstallLink(link) {
    if (!miModal) miBuild();
    if (MI.step === "get") backend.call("market_cancel");
    miDiscard();
    const run = ++MI.run;
    Object.assign(MI, { link, step: "look", info: null, pack: null, error: "", profileError: "", got: 0, total: 0 });
    if (miModal.hidden) { miReturn = document.activeElement; miModal.hidden = false; }
    backend.showWindow();
    miRender();
    try {
      MI.info = await backend.run("market_lookup", { link });
      if (run !== MI.run) return;
      MI.step = "get";
      MI.total = MI.info.size;
      miRender();
      const path = await backend.run("market_download", { link });
      if (run !== MI.run) { backend.call("market_discard", { path }); return; }
      MI.path = path;
      MI.pack = await backend.run("inspect_plugin", { path });
      if (run !== MI.run) return;
      MI.step = "ready";
    } catch (e) {
      if (run !== MI.run) return;
      MI.step = "err";
      MI.error = String(e);
    }
    miRender();
  }

  async function miInstall() {
    const run = MI.run, name = MI.info.name;
    MI.step = "busy";
    miRender();
    try {
      applyPluginList(await backend.run("install_plugin", { path: MI.path }));
      resetShown();
      miDiscard();
      if (run !== MI.run) return;
      MI.step = "done";
      say(`Installed ${name}`);
    } catch (e) {
      if (run !== MI.run) return;
      MI.step = "err";
      MI.error = String(e);
    }
    await loadPluginConfig();
    if (A.view === "settings") renderSettings();
    miRender();
  }

  // The profile that comes with the plugin: downloaded, checked, then opened by the same import as a
  // double-clicked profile file
  async function miProfile() {
    const run = MI.run;
    let path = "";
    MI.step = "pbusy";
    MI.profileError = "";
    miRender();
    try {
      path = await backend.run("market_profile_download", { link: MI.link });
      if (run !== MI.run) return;
      miClose();
      await importProfileFile(path);
    } catch (e) {
      if (run !== MI.run) return;
      MI.step = "done";
      MI.profileError = String(e);
      miRender();
    } finally {
      if (path) backend.call("market_discard", { path });
    }
  }

  // Progress for the download in the popup; only the bar and its words change
  backend.listen("market_progress", (e) => {
    if (MI.step !== "get" || !MI.info || e.id !== MI.info.id) return;
    MI.got = e.got;
    MI.total = e.total || MI.total;
    const bar = document.getElementById("mi-fill"), words = document.getElementById("mi-got");
    const pct = MI.total ? Math.min(100, Math.round((MI.got / MI.total) * 100)) : 0;
    if (bar) bar.style.width = `${pct}%`;
    if (words) words.textContent = `${mb(MI.got)} of ${mb(MI.total)} · ${pct}%`;
  });

  function miRender() {
    const body = document.getElementById("mi-body");
    body.textContent = "";
    const i = MI.info;
    if (i) {
      const head = el("div", { className: "mi-head" });
      head.append(el("b", { className: "mi-name", textContent: `${i.name} ${i.version}` }));
      const facts = [i.official ? "Official" : "Community", i.author ? `by ${i.author}` : "", `${i.keys} ${i.keys === 1 ? "key" : "keys"}`, mb(i.size)].filter(Boolean);
      head.append(el("p", { className: "muted", textContent: facts.join(" · ") }));
      if (i.summary) head.append(el("p", { textContent: i.summary }));
      body.append(head);
      if (i.needs && i.needs.length) body.append(el("p", { className: "help", textContent: `Needs: ${i.needs.join("; ")}` }));
      if (i.installed != null) {
        const c = verCmp(i.version, i.installed);
        const t = c === 0 ? `Version ${i.installed} is already installed. Installing puts a fresh copy in.`
          : c > 0 ? `Updates version ${i.installed} you have now.`
          : `You have the newer version ${i.installed}. This puts the older one back.`;
        body.append(el("p", { className: "help", textContent: t }));
      }
    }
    const row = (...kids) => el("div", { className: "row" }, kids);
    if (MI.step === "look") {
      body.append(el("p", { className: "mi-status", textContent: "Looking it up in the marketplace on GitHub…" }), el("div", { className: "mi-bar busy" }, [el("i")]));
      body.append(row(btn("Cancel", "btn", miClose)));
    } else if (MI.step === "get") {
      const pct = MI.total ? Math.min(100, Math.round((MI.got / MI.total) * 100)) : 0;
      const fill = el("i", { id: "mi-fill" });
      fill.style.width = `${pct}%`;
      const bar = el("div", { className: "mi-bar" }, [fill]);
      bar.setAttribute("role", "progressbar");
      bar.setAttribute("aria-label", "Download");
      body.append(el("p", { className: "mi-status", textContent: "Downloading from GitHub" }), bar, el("p", { className: "muted num", id: "mi-got", textContent: `${mb(MI.got)} of ${mb(MI.total)} · ${pct}%` }));
      body.append(row(btn("Cancel", "btn", miClose)));
    } else if (MI.step === "ready") {
      const p = MI.pack;
      body.append(el("p", { className: "mi-status ok", textContent: "Downloaded, and it matches the marketplace's checksum." }));
      body.append(el("p", { className: "muted", textContent: `${p.actions} ${p.actions === 1 ? "action" : "actions"} · Signed by: ${signedBy(p.signature)}` }));
      body.append(el("p", { className: "help", textContent: "Plugins run with the same rights as Minniondeck. Install only plugins from people you trust." }));
      if (!p.runs && p.why) body.append(el("p", { className: "warn", textContent: `It won't run yet. ${p.why}` }));
      const yes = btn("Install", "btn go", miInstall);
      yes.disabled = !!(p.signature && p.signature.trust === "bad");
      body.append(row(yes, btn("Cancel", "btn", miClose)));
      setTimeout(() => yes.focus(), 30);
    } else if (MI.step === "busy") {
      body.append(el("p", { className: "mi-status", textContent: "Installing…" }), el("div", { className: "mi-bar busy" }, [el("i")]));
    } else if (MI.step === "done") {
      body.append(el("p", { className: "mi-status ok", textContent: `${i.name} ${i.version} is installed. Its keys are in the action list.` }));
      const open = btn("Open Plugins", "btn", () => { miClose(); window.openPluginSettings(); });
      if (i.profile) {
        const n = i.profile.pages;
        body.append(el("p", { className: "help", textContent: `It comes with a ready-made profile: ${i.profile.name} (${n} ${n === 1 ? "page" : "pages"}). Add it to your profiles?` }));
        if (MI.profileError) body.append(el("p", { className: "warn", textContent: MI.profileError }));
        const add = btn("Add the profile", "btn go", miProfile);
        body.append(row(add, btn("Not now", "btn", miClose), open));
        setTimeout(() => add.focus(), 30);
      } else {
        const ok = btn("Done", "btn go", miClose);
        body.append(row(ok, open));
        setTimeout(() => ok.focus(), 30);
      }
    } else if (MI.step === "pbusy") {
      body.append(el("p", { className: "mi-status", textContent: "Getting the profile…" }), el("div", { className: "mi-bar busy" }, [el("i")]));
    } else if (MI.step === "err") {
      body.append(el("p", { className: "warn", textContent: MI.error }));
      const again = btn("Try again", "btn go", () => openInstallLink(MI.link));
      body.append(row(again, btn("Close", "btn", miClose)));
      setTimeout(() => again.focus(), 30);
    }
    // Keep focus inside the popup while it waits (Cancel is the one thing to press)
    if ((MI.step === "look" || MI.step === "get") && !miModal.contains(document.activeElement)) {
      const b = body.querySelector("button");
      if (b) setTimeout(() => b.focus(), 30);
    }
  }
