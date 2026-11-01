  // ===== Rail =====
  UI_GLYPHS.brush = [".....###", "....####", "...####.", "..####..", ".####...", ".##.....", "##......", "#......."];
  // GIFs and pictures are made on the website (openSite); they come in with Import in the Edit button window
  const VIEWS = [["deck", "Deck", "grid"], ["profiles", "Profiles", "people"], ["phones", "Phones", "phone"], ["marketplace", "Marketplace", "shop"], ["settings", "Settings", "gear"]];
  function renderRail() {
    const rail = $("a-rail");
    rail.innerHTML = "";
    VIEWS.forEach(([id, label, icon], i) => {
      if (id === "settings") rail.appendChild(el("span", { className: "sp" }));
      const b = el("button", { type: "button", id: `ar-${id}` });
      b.setAttribute("role", "tab");
      b.setAttribute("aria-selected", String(A.view === id));
      b.setAttribute("aria-controls", `av-${id}`);
      b.tabIndex = A.view === id ? 0 : -1;
      b.innerHTML = `${glyph(icon, 24)}<span>${label}</span>`;
      if (id === "phones") {
        const n = latest() ? PHONES.filter((p) => p.appVersion && p.appVersion !== latest()).length : 0;
        if (n) { const badge = el("span", { className: "badge", textContent: String(n) }); badge.setAttribute("aria-hidden", "true"); b.appendChild(badge); b.setAttribute("aria-label", `Phones, ${n} ${n === 1 ? "needs" : "need"} an update`); }
      }
      // Plugin updates from the marketplace, and a new Minniondeck from GitHub Releases (12c-updates.js)
      const waiting = id === "marketplace" ? pluginUpdates().length : id === "settings" && appUpdateWaiting() ? 1 : 0;
      if (waiting) {
        const badge = el("span", { className: "badge", textContent: String(waiting) });
        badge.setAttribute("aria-hidden", "true");
        b.appendChild(badge);
        b.setAttribute("aria-label", id === "settings" ? "Settings, a Minniondeck update is out" : `Marketplace, ${waiting} ${waiting === 1 ? "plugin update" : "plugin updates"}`);
      }
      b.addEventListener("click", () => go(id));
      rail.appendChild(b);
    });
  }
  $("a-rail").addEventListener("keydown", (e) => {
    const keys = { ArrowDown: 1, ArrowRight: 1, ArrowUp: -1, ArrowLeft: -1 };
    if (!(e.key in keys)) return;
    e.preventDefault();
    const i = VIEWS.findIndex(([id]) => id === A.view);
    const next = VIEWS[(i + keys[e.key] + VIEWS.length) % VIEWS.length][0];
    go(next);
    $(`ar-${next}`).focus();
  });
  function go(view) {
    if (A.view === "settings" && view !== "settings") backend.call("save_settings", { settings: SET });
    // A pairing code only works while the Phones page is open
    if (A.view === "phones" && view !== "phones") endPairing();
    A.view = view;
    A.profMenu = false;
    VIEWS.forEach(([id]) => { $(`av-${id}`).hidden = id !== view; });
    renderRail();
    renderView();
    renderStatus();
  }
  function renderView() {
    if (A.view === "deck") renderDeck();
    if (A.view === "profiles") renderProfiles();
    if (A.view === "phones") renderPhones();
    if (A.view === "marketplace") renderMarketplace();
    if (A.view === "settings") renderSettings();
  }

