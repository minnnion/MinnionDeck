  // The window has no status bar; this keeps the title in step
  function renderStatus() { renderCrumb(); }

  // Keep the window in step with the editor, the crew and the theme button
  const refresh = () => setTimeout(() => { if (A.view === "deck") { renderMain(); renderInsp(); renderTree(); } renderStatus(); }, 0);
  $("ed-form").addEventListener("submit", refresh);
  $("ed-form").addEventListener("click", (e) => { if (e.target.closest(".ed-foot")) refresh(); });
  $("crew").addEventListener("click", () => setTimeout(() => { renderStatus(); if (A.view === "settings") renderSettings(); }, 1500));
  $("hire-section").addEventListener("click", () => setTimeout(renderStatus, 1600));
  $("theme-toggle").addEventListener("click", () => { if (A.view === "settings" && settingsTab === "appearance") renderSettings(); });

  // The app's logo: the same 16 × 16 minnion as the tray and app icon (src-tauri/src/tray.rs).
  // The title bar is dark in both themes, so the outline is the light one, as on a dark taskbar.
  const APP_ICON = [
    "......rr........", "......rr........", ".......o........", ".....oooooo.....",
    "...oommmmmmoo...", "..ommmmmmmmmmo..", "..omwwmmmmwwmo..", ".ommwpmmmmwpmmo.",
    ".ommmmmmmmmmmmo.", ".ommmmmoommmmdo.", ".ommmmmmmmmmmdo.", "..ommmmmmmmmdo..",
    "...oommmmmmoo...", ".....oooooo.....", ".....oo..oo.....", ".....oo..oo....."
  ];
  function appIconSVG(px) {
    const C = { o: "#e4eaf6", m: "#5fd0a8", d: "#3aa585", w: "#ffffff", p: "#1e2433", r: "#ff5a5f" };
    let r = "";
    APP_ICON.forEach((row, y) => [...row].forEach((c, x) => { if (C[c]) r += `<rect x="${x}" y="${y}" width="1" height="1" fill="${C[c]}"/>`; }));
    return `<svg viewBox="0 0 16 16" width="${px}" height="${px}" shape-rendering="crispEdges" aria-hidden="true">${r}</svg>`;
  }
  $("a-logo").innerHTML = appIconSVG(16);
  // Loads saved profiles and settings first, then opens the Deck (see 11-persistence.js)
  startApp();
  startUpdateChecks();
