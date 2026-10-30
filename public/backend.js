// Bridge between the UI and the Rust backend.
// Inside Tauri every call goes to a Rust command; in a plain browser (npm run dev) the
// fallback value is used, so the UI behaves exactly like the original mockup either way.
(() => {
  const tauri = () => window.__TAURI__ && window.__TAURI__.core;

  async function call(cmd, args = {}, fallback = null) {
    const t = tauri();
    if (!t) return fallback;
    try {
      return await t.invoke(cmd, args);
    } catch (err) {
      console.warn(`[backend] ${cmd} failed:`, err);
      return fallback;
    }
  }

  // Run a command that answers with a message, and show it in the toast.
  function report(cmd, args, fallback) {
    // `say` is the UI's own toast helper (a top-level const in the page scripts).
    call(cmd, args, fallback).then((msg) => say(msg || fallback));
  }

  // Events the backend sends, such as phones_changed. Does nothing outside the desktop app.
  function listen(name, fn) {
    const ev = window.__TAURI__ && window.__TAURI__.event;
    if (ev) ev.listen(name, (e) => fn(e.payload));
  }

  // Like call, but a failure is thrown with the backend's message, for things the user is told about
  async function run(cmd, args = {}) {
    const t = tauri();
    if (!t) throw "This works in the Minniondeck desktop app.";
    return t.invoke(cmd, args);
  }

  const win = () => window.__TAURI__ && window.__TAURI__.window.getCurrentWindow();

  // Saved pictures and GIF files are shown straight from the library's media folder
  function mediaUrl(dir, file) {
    const t = tauri();
    if (!t || !dir || !file) return "";
    return t.convertFileSrc(`${dir}\\${file}`);
  }

  // The window starts hidden and is shown once saved data is on screen
  function showWindow() {
    const w = win();
    if (w) w.show();
  }

  async function saveNow() {
    if (!window.mdFlush) return;
    try { await window.mdFlush(); } catch (e) { console.warn("[save] could not save:", e); }
  }

  // Closing the window (✕, Alt+F4, the taskbar) saves first. With "Keep running" (Settings >
  // General) it then only hides, and the minnion hops in the tray; with "Quit" the app closes.
  // The persistence code sets mdFlush and mdCloseToTray.
  function wireClose() {
    const w = win();
    if (!w) return;
    w.onCloseRequested(async (event) => {
      await saveNow();
      if (window.mdCloseToTray && window.mdCloseToTray()) {
        event.preventDefault();
        await w.hide();
        call("tray_hop");
      }
    });
    // Quit from the tray menu: save, then close for good
    listen("app-quit-requested", async () => {
      await saveNow();
      call("quit_app");
    });
  }

  // A .mdprofile opened while the app is running (double-click in Explorer)
  function wireOpenFile() {
    const ev = window.__TAURI__ && window.__TAURI__.event;
    if (ev) ev.listen("open-profile-file", (e) => { if (window.mdOpenProfileFile) window.mdOpenProfileFile(e.payload); });
  }

  // The window's own title bar: drag to move, double-click to maximise, and the three buttons.
  function wireTitlebar() {
    const bar = document.querySelector(".appwin > .titlebar");
    if (!bar) return;
    bar.querySelectorAll("[data-win]").forEach((b) => {
      b.addEventListener("mousedown", (e) => e.stopPropagation());
      b.addEventListener("click", () => {
        const w = win();
        if (!w) return;
        if (b.dataset.win === "min") w.minimize();
        if (b.dataset.win === "max") w.toggleMaximize();
        if (b.dataset.win === "close") w.close();
      });
    });
  }

  // Files dragged onto the window from the desktop or File Explorer. Windows gives their full
  // paths; fn(type, paths, x, y) gets "enter", "over", "drop" or "leave" and page coordinates.
  function onFileDrop(fn) {
    const wv = window.__TAURI__ && window.__TAURI__.webview;
    if (!wv) return;
    wv.getCurrentWebview().onDragDropEvent((e) => {
      const p = e.payload || {}, r = window.devicePixelRatio || 1;
      fn(p.type, p.paths || [], p.position ? p.position.x / r : 0, p.position ? p.position.y / r : 0);
    });
  }

  window.backend = { call, run, say: report, listen, mediaUrl, showWindow, onFileDrop, available: () => !!tauri() };
  wireClose();
  wireOpenFile();
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", wireTitlebar);
  else wireTitlebar();
})();
