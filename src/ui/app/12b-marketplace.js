  // ===== Marketplace =====
  // The plugins in the Minnion Marketplace (github.com/minnnion/Minnion-Marketplace), read live from its
  // index.json by market_list (src-tauri/src/market.rs). It looks like the website's marketplace.
  // Install opens the same popup as the website's Install button (10c-market-install.js), which
  // downloads the plugin from GitHub and checks it before installing.
  UI_GLYPHS.shop = ["..####..", ".#....#.", ".#....#.", "########", "#......#", "#.#..#.#", "#......#", "########"];
  UI_GLYPHS.mkSearch = [".###....", "#...#...", "#...#...", "#...#...", ".####...", ".....#..", "......##", "......##"];
  const MK_INDEX = "https://raw.githubusercontent.com/minnnion/Minnion-Marketplace/main/index.json";
  // Pictures in the list are full GitHub addresses or paths on the website; nothing else is shown
  const MK_SITE = "https://minnnion.github.io/";
  const MK_HOSTS = ["raw.githubusercontent.com", "minnnion.github.io"];
  // The same four uses as the website and Minniondeck Setup
  const MK_CATS = [["all", "All"], ["general", "General"], ["streaming", "Streaming"], ["games", "Games"], ["work", "Work"]];
  const MK_CATNAME = { general: "General", streaming: "Streaming", games: "Games", work: "Work" };
  const MK = { data: null, error: "", loading: false, cat: "all", q: "", sort: "name", open: null };

  const mkSize = (b) => (b >= 1048576 ? `${(b / 1048576).toFixed(1)} MB` : `${Math.max(1, Math.round(b / 1024))} KB`);
  const mkKeys = (n) => `${n} ${n === 1 ? "key" : "keys"}`;
  function mkPic(src) {
    try {
      const u = new URL(String(src || ""), MK_SITE);
      return u.protocol === "https:" && MK_HOSTS.includes(u.hostname) ? u.href : "";
    } catch { return ""; }
  }
  function mkImg(src, cls) {
    const url = mkPic(src);
    const img = el("img", { className: cls || "", alt: "", loading: "lazy", decoding: "async" });
    if (url) img.src = url;
    img.addEventListener("error", () => { img.style.visibility = "hidden"; });
    return img;
  }
  const mkChip = (text, cls) => el("span", { className: `mk-chip${cls ? ` ${cls}` : ""}`, textContent: text });
  const mkInstalled = (p) => plugins.find((x) => x.id === p.id);
  const mkLink = (p, v) => `minniondeck://install?id=${encodeURIComponent(p.id)}${v && v !== p.version ? `&v=${encodeURIComponent(v)}` : ""}`;
  // A plugin's entry with every list it uses, so a short entry in index.json can't break the page
  const mkEntry = (p) => ({ ...p, actions: p.actions || [], needs: p.needs || [], settings: p.settings || [], shots: p.shots || [], versions: p.versions && p.versions.length ? p.versions : [{ v: p.version, size: p.size, sha256: p.sha256 }] });

  async function marketLoad(force) {
    if (MK.loading || (MK.data && !force)) return;
    MK.loading = true;
    MK.error = "";
    if (A.view === "marketplace") renderMarketplace();
    try {
      // The desktop app reads it in Rust; a browser preview of the page reads it straight from GitHub
      const d = backend.available() ? await backend.run("market_list") : await fetch(MK_INDEX, { cache: "no-cache" }).then((r) => { if (!r.ok) throw new Error(`GitHub answered ${r.status}`); return r.json(); });
      MK.data = { ...d, plugins: (d.plugins || []).map(mkEntry) };
    } catch (e) {
      MK.error = String(e && e.message ? e.message : e);
    }
    MK.loading = false;
    if (A.view === "marketplace") renderMarketplace();
    renderRail();
  }

  function mkInstallButton(p, v, small) {
    const have = mkInstalled(p);
    const ver = v || p.version;
    let text = small ? "Install" : `Install v${ver}`;
    let cls = "btn mk-go";
    if (have && !v) {
      const c = verCmp(p.version, have.version || "0");
      if (c === 0) { text = "Installed"; cls = "btn mk-done"; }
      else if (c > 0) text = `Update to v${p.version}`;
    }
    const b = btn(text, `${cls}${small ? " sm" : ""}`, () => openInstallLink(mkLink(p, ver)));
    b.setAttribute("aria-label", `${text}: ${p.name}${v ? ` ${v}` : ""}`);
    return b;
  }

  // One plugin card, as the website draws it
  function mkCard(p, n) {
    const card = el("article", { className: "mk-plug" });
    card.style.animationDelay = `${n * 0.06}s`;
    const shot = el("button", { type: "button", className: "mk-shot" });
    shot.setAttribute("aria-label", `${p.name} details`);
    shot.style.setProperty("--c", `var(--${p.color || "sky"})`);
    if (p.shots.length >= 4) shot.append(el("div", { className: "mk-grid8" }, p.shots.slice(0, 8).map((s) => mkImg(s, "px"))));
    else shot.append(el("div", { className: "mk-solo" }, [mkImg(p.icon, "px"), el("div", { className: "mk-bars" }, [el("i"), el("i"), el("i"), el("i"), el("i")])]));
    shot.addEventListener("click", () => mkGo(p.id));
    const have = mkInstalled(p);
    const meta = el("div", { className: "mk-meta" }, [
      p.official ? mkChip("Official", "ok") : mkChip("Community"),
      mkChip(MK_CATNAME[p.category] || p.category || "General"),
      mkChip(`v${p.version}`),
      mkChip(mkKeys(p.actions.length)),
    ]);
    if (have) meta.append(mkChip(`You have v${have.version}`, "have"));
    const body = el("div", { className: "mk-body" }, [
      el("h3", {}, [mkImg(p.icon, "px"), el("span", { textContent: p.name })]),
      el("p", { textContent: p.summary || "" }),
      meta,
      el("div", { className: "mk-acts" }, [btn("Details", "btn sm", () => mkGo(p.id)), mkInstallButton(p, null, true)]),
    ]);
    card.append(shot, body);
    return card;
  }

  function mkShowList() {
    const grid = $("mk-grid"), cats = $("mk-cats");
    if (!grid || !MK.data) return;
    const all = MK.data.plugins, q = MK.q.trim().toLowerCase();
    cats.textContent = "";
    // Updates is only there while an installed plugin has a newer version
    const upd = pluginUpdates();
    if (!upd.length && MK.cat === "updates") MK.cat = "all";
    [...MK_CATS, ...(upd.length ? [["updates", "Updates"]] : [])].forEach(([id, name]) => {
      const n = id === "all" ? all.length : id === "updates" ? upd.length : all.filter((p) => p.category === id).length;
      const b = el("button", { type: "button" }, [name, el("small", { textContent: String(n) })]);
      b.setAttribute("aria-pressed", String(MK.cat === id));
      b.addEventListener("click", () => { MK.cat = id; mkShowList(); });
      cats.append(b);
    });
    const list = all.filter((p) => {
      if (MK.cat === "updates") { if (!upd.includes(p)) return false; } else if (MK.cat !== "all" && p.category !== MK.cat) return false;
      if (!q) return true;
      return [p.name, p.summary, p.id, MK_CATNAME[p.category], ...p.actions].join(" ").toLowerCase().includes(q);
    });
    list.sort((a, b) => (MK.sort === "keys" ? b.actions.length - a.actions.length : MK.sort === "size" ? a.size - b.size : a.name.localeCompare(b.name)));
    $("mk-count").textContent = list.length === 1 ? "1 plugin" : `${list.length} plugins`;
    grid.textContent = "";
    if (!list.length) {
      const box = el("div", { className: "mk-empty mk-frame" });
      if (MK.cat !== "all" && MK.cat !== "general" && !q) {
        const w = MK_CATNAME[MK.cat].toLowerCase();
        box.append(mkChip("Coming soon", "soon"), el("h3", { textContent: `No ${w} plugins yet` }), el("p", { textContent: `Official ${w} plugins are on the way. You can also build one and send it in.` }), btn("Submit a plugin", "btn sm", () => backend.call("open_site", { page: "marketplace" })));
      } else {
        box.append(el("h3", { textContent: `Nothing matches “${MK.q}”` }), el("p", { textContent: "Try a plugin name or a key, like “volume” or “timer”." }), btn("Clear search", "btn sm", () => { MK.q = ""; $("mk-q").value = ""; mkShowList(); $("mk-q").focus(); }));
      }
      grid.append(box);
      return;
    }
    list.forEach((p, n) => grid.append(mkCard(p, n)));
  }

  function mkShowDetail(box, id) {
    const back = btn("← All plugins", "mk-back", () => mkGo(null));
    const p = MK.data.plugins.find((x) => x.id === id);
    if (!p) {
      box.append(back, el("div", { className: "mk-empty mk-frame" }, [el("h3", { textContent: "That plugin isn't in the list" }), el("p", { textContent: "It may have been renamed or removed." })]));
      return;
    }
    const shot = el("div", { className: "mk-pd-shot mk-frame" });
    shot.style.setProperty("--c", `var(--${p.color || "sky"})`);
    const keys = el("div", { className: "mk-pd-keys" });
    (p.shots.length ? p.shots : [p.icon]).forEach((s, n) => { const k = el("span", {}, [mkImg(s, "px")]); k.style.animationDelay = `${n * 0.035}s`; keys.append(k); });
    shot.append(keys);
    const have = mkInstalled(p);
    const meta = el("div", { className: "mk-meta" }, [mkChip(`v${p.version}`), mkChip(mkKeys(p.actions.length))]);
    if (p.api != null) meta.append(mkChip(`Plugin API ${p.api}`));
    if (p.author) meta.append(mkChip(`By ${p.author}`));
    if (p.profile) meta.append(mkChip("Includes a profile", "ok"));
    if (have) meta.append(mkChip(`You have v${have.version}`, "have"));
    const hash = el("div", { className: "mk-hash" }, ["SHA-256", el("code", { textContent: p.sha256 || "", title: p.sha256 || "" })]);
    const info = el("div", { className: "mk-pd-info" }, [
      el("span", { className: "mk-label", textContent: `${MK_CATNAME[p.category] || p.category || "General"} · ${p.official ? "Official" : "Community"}` }),
      el("h2", {}, [mkImg(p.icon, "px"), el("span", { textContent: p.name })]),
      el("p", { textContent: p.about || p.summary || "" }),
      meta,
      el("div", { className: "mk-get" }, [mkInstallButton(p), el("span", { className: "mk-count", textContent: `${mkSize(p.size)} from GitHub, checked before it installs` })]),
      hash,
    ]);
    const li = (t) => el("li", { textContent: t });
    const vers = el("tbody");
    p.versions.forEach((v, n) => {
      const first = el("td", { textContent: `v${v.v}` });
      if (n === 0) first.append(" ", mkChip("Latest", "ok"));
      vers.append(el("tr", {}, [first, el("td", { textContent: v.size ? mkSize(v.size) : "" }), el("td", {}, [mkInstallButton(p, v.v, true)])]));
    });
    const boxes = el("div", { className: "mk-pd-grid" }, [
      el("div", { className: "mk-box mk-frame" }, [el("h3", { textContent: "Keys it adds" }), el("div", { className: "mk-tags" }, p.actions.map((a) => mkChip(a)))]),
      el("div", { className: "mk-box mk-frame" }, [
        el("h3", { textContent: "What you need" }), el("ul", {}, p.needs.length ? p.needs.map(li) : [li("Nothing else")]),
        el("h3", { textContent: "Settings" }), el("ul", {}, p.settings.length ? p.settings.map(li) : [li("None, it just works")]),
      ]),
      el("div", { className: "mk-box mk-frame" }, [el("h3", { textContent: "Versions" }), el("table", { className: "mk-vers" }, [vers])]),
    ]);
    box.append(back, el("div", { className: "mk-pd" }, [shot, info]), boxes);
    if (p.profile) {
      const n = p.profile.pages || 1;
      box.append(el("div", { className: "mk-box mk-frame" }, [el("h3", { textContent: "Comes with a profile" }), el("p", { textContent: `${p.profile.name}, ${n} ${n === 1 ? "page" : "pages"}. After the plugin installs, Minniondeck offers to add it to your profiles, so the buttons are ready.` })]));
    }
  }

  function mkGo(id) {
    MK.open = id;
    renderMarketplace();
    $("av-marketplace").scrollTop = 0;
    const f = $("av-marketplace").querySelector(id ? ".mk-back" : "#mk-q");
    if (f) f.focus({ preventScroll: true });
  }

  function renderMarketplace() {
    const box = $("av-marketplace");
    box.textContent = "";
    const pad = el("div", { className: "vpad mk" });
    box.append(pad);
    if (!MK.data) {
      const msg = el("div", { className: "mk-msg mk-frame" });
      if (MK.error) msg.append(el("p", { textContent: `The plugin list didn't load. ${MK.error}` }), btn("Try again", "btn sm", () => marketLoad(true)));
      else msg.append(el("div", { className: "mk-ldr" }, [el("i"), el("i"), el("i")]), el("p", { textContent: "Reading the plugin list from GitHub…" }));
      pad.append(el("div", { className: "mk-top" }, [el("span", { className: "mk-label", textContent: "Marketplace" }), el("h1", { textContent: "Plugins for your deck" })]), msg);
      if (!MK.loading && !MK.error) marketLoad();
      return;
    }
    if (MK.open) { mkShowDetail(pad, MK.open); return; }
    const top = el("div", { className: "mk-top" }, [
      el("span", { className: "mk-label", textContent: "Marketplace" }),
      el("h1", { textContent: "Plugins for your deck" }),
      el("p", { textContent: "Every plugin here is free. Press Install and Minniondeck downloads the plugin from GitHub, checks it and asks before installing." }),
    ]);
    const q = el("input", { type: "search", id: "mk-q", placeholder: "Search plugins and keys", value: MK.q });
    q.setAttribute("aria-label", "Search plugins");
    q.addEventListener("input", () => { MK.q = q.value; mkShowList(); });
    const sort = el("select", { id: "mk-sort" }, [["name", "Name"], ["keys", "Most keys"], ["size", "Smallest"]].map(([v, t]) => el("option", { value: v, textContent: t })));
    sort.value = MK.sort;
    sort.addEventListener("change", () => { MK.sort = sort.value; mkShowList(); });
    const cats = el("div", { className: "mk-cats", id: "mk-cats" });
    cats.setAttribute("role", "group");
    cats.setAttribute("aria-label", "Category");
    const search = el("label", { className: "mk-search" });
    search.innerHTML = glyph("mkSearch", 18);
    search.append(q);
    const tools = el("div", { className: "mk-tools mk-frame" }, [search, cats, el("label", { className: "mk-sort" }, ["Sort", sort])]);
    const count = el("p", { className: "mk-count", id: "mk-count" });
    count.setAttribute("aria-live", "polite");
    const foot = el("div", { className: "mk-foot" }, [
      el("span", { className: "mk-count", textContent: MK.data.updated ? `List updated ${MK.data.updated}` : "" }),
      btn("Refresh", "btn sm", () => marketLoad(true)),
      btn("Submit a plugin", "btn sm", () => backend.call("open_site", { page: "marketplace" })),
    ]);
    pad.append(top, tools, el("div", { className: "mk-row" }, [count, foot]), el("div", { className: "mk-plugs", id: "mk-grid" }));
    mkShowList();
  }
