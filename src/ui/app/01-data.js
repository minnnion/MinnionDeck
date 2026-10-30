  UI_GLYPHS.grid = ["###.###.", "###.###.", "###.###.", "........", "###.###.", "###.###.", "###.###.", "........"];
  UI_GLYPHS.phone = [".######.", ".#....#.", ".#....#.", ".#....#.", ".#....#.", ".######.", ".##..##.", ".######."];
  UI_GLYPHS.people = ["..##....", ".####.##", ".####.##", "..##.###", ".####...", "######..", "######..", "........"];

  // ----- More pages for the example profiles -----
  Object.assign(boards, {
    Chat: [T("c-hi", "Say hi", "chat", 2, 1, "single", [S("text", { text: "Hi chat!" })], look("lilac")), T("c-mark", "Screenshot", "flag", 1, 1, "single", [S("key", { keys: "Win+Alt+PrtSc" })]), T("c-clip", "Clip it", "rewind", 1, 1, "single", [S("key", { keys: "Win+Alt+G" })])],
    Music: [T("m-play", "Play", "playpause", 1, 1, "single", [S("key", { keys: "PlayPause" })], look("sky")), T("m-next", "Next", "skip", 1, 1, "single", [S("key", { keys: "NextTrack" })]), T("m-like", "Previous", "rewind", 1, 1, "single", [S("key", { keys: "PreviousTrack" })], look("peach")), T("m-up", "Louder", "up", 1, 1, "single", [S("vol", { direction: "Up", amount: "5" })]), T("m-down", "Quieter", "down", 1, 1, "single", [S("vol", { direction: "Down", amount: "5" })])],
    Meetings: [T("w-mute", "Mute", "mic", 1, 1, "single", [S("mic")], look("tile", { onBg: "coral" })), T("w-join", "Join call", "globe", 2, 1, "single", [S("url", { url: "https://meet.example.com" })], look("mint")), T("w-cam", "Camera", "camera", 1, 1, "single", [S("key", { keys: "Ctrl+E" })]), T("w-share", "Share screen", "monitor", 1, 1, "single", [S("key", { keys: "Ctrl+Shift+E" })])],
    Apps: [T("a-web", "Browser", "globe", 1, 1, "single", [S("url", { url: "https://example.com" })]), T("a-notes", "Notes", "folder", 1, 1, "single", [S("run", { path: "C:\\Windows\\notepad.exe" })]), T("a-term", "Terminal", "app", 1, 1, "single", [S("run", { path: "wt.exe" })]), T("a-lock", "Lock PC", "lock", 1, 1, "single", [S("key", { keys: "Win+L" })], look("sun"))],
    Design: [T("d-shot", "Screenshot", "camera", 1, 1, "single", [S("key", { keys: "Win+Shift+S" })]), T("d-files", "Project files", "folder", 2, 1, "single", [S("run", { path: "C:\\Projects" })], look("peach")), T("d-focus", "Focus", "star", 1, 1, "single", [S("delay", { ms: "1000" })])],
    Game: [T("g-dmute", "Mute mic", "headset", 1, 1, "single", [S("mic")], look("tile", { onBg: "coral" })), T("g-clip", "Save clip", "rewind", 2, 1, "single", [S("key", { keys: "Win+Alt+G" })], look("lilac")), T("g-quiet", "Quieter", "down", 1, 1, "single", [S("vol", { direction: "Down", amount: "10" })])]
  });

  // More example GIFs and widgets
  findTile("m-play").states[0].gif = { id: "nowplaying", place: "bg", speed: "normal" };
  findTile("w-join").states[0].gif = { id: "clock", place: "bg", speed: "normal" };
  findTile("g-clip").states[0].gif = { id: "matrix", place: "bg", speed: "normal" };
  findTile("c-hi").states[0].gif = { id: "viewers", place: "bg", speed: "normal" };
  findTile("d-focus").states[0].gif = { id: "minnion", place: "icon", speed: "normal" };
  findTile("a-lock").states[0].gif = { id: "stars", place: "bg", speed: "slow" };

  // ----- Profiles, folders and pages -----
  const P = (id, name, board, cols = 4, rows = 3) => ({ kind: "page", id, name, board, cols, rows });
  const F = (id, name, open, children) => ({ kind: "folder", id, name, open, children });
  // A first start has one empty profile, not the example ones (Streaming, Work, Gaming): those
  // became the first save and stayed. Saved data, an import or a new profile replaces it.
  Object.keys(boards).forEach((k) => delete boards[k]);
  boards["Board-1"] = [];
  const PROFILES = [
    { id: "mine", name: "My profile", body: "sky", icon: "star", auto: "Never", tree: [P("p-1", "Page 1", "Board-1", 5, 3)] }
  ];
  const APPS = ["Never", "OBS Studio", "Microsoft Teams", "Steam", "Spotify", "Visual Studio Code", "Any full-screen game"];
  const A = { view: "deck", prof: "mine", page: "p-1", tile: null, editLayout: false, freshNode: null, freshCard: null, renaming: null, confirmDel: false, profMenu: false };
  let seq = 0;
  const uid = (p) => `${p}${Date.now().toString(36)}${seq++}`;
  const prof = () => PROFILES.find((p) => p.id === A.prof);
  function walk(nodes, fn, path = []) { nodes.forEach((n) => { fn(n, path); if (n.kind === "folder") walk(n.children, fn, [...path, n]); }); }
  function pagesOf(p) { const out = []; walk(p.tree, (n, path) => { if (n.kind === "page") out.push({ page: n, path }); }); return out; }
  function findNode(id, p = prof()) { let hit = null; walk(p.tree, (n, path) => { if (n.id === id) hit = { node: n, path }; }); return hit; }
  function findPageAny(id) { for (const p of PROFILES) { const h = findNode(id, p); if (h) return { ...h, prof: p }; } return null; }
  const curPage = () => (findNode(A.page) || {}).node;
  const tilesOf = (page) => boards[page.board] || [];

  // ----- Phones -----
  // Paired phones come from the backend (phones/hub.rs) and update live as they connect and leave.
  const PHONES = [];
  // The newest phone app version. Unknown until the update source is set up.
  const latest = () => null;
  let previewPhone = null, removing = null;

  // ----- Settings values -----
  const SET = { theme: "system", startWin: true, tray: false, close: "tray", lang: "English", anim: "full", crew: true, tileSize: "m", names: true, autoSwitch: true, backBtn: true, startPage: "last", conn: "wifi", port: "47800", pin: true, followPc: true, awake: true, haptics: true, pluginsPath: "C:\\Users\\you\\Documents\\Minniondeck\\plugins", plugUpd: true, autoBackup: true, keep: "5", backupPath: "C:\\Users\\you\\Documents\\Minniondeck\\backups", appChannel: "stable", bundlePhone: true };
  let settingsTab = "general", checkState = "idle";
  window.openPluginSettings = () => { hideEditor(); settingsTab = "plugins"; go("settings"); };

  const el = (tag, props = {}, kids = []) => { const e = Object.assign(document.createElement(tag), props); kids.forEach((k) => e.append(k)); return e; };
  const btn = (text, cls, fn, aria) => { const b = el("button", { type: "button", className: cls, textContent: text }); if (aria) b.setAttribute("aria-label", aria); if (fn) b.addEventListener("click", fn); return b; };
  // p.body comes from the profile (an imported one carries whatever its file says), so it goes
  // through cssColour before it reaches the style attribute
  const chipFor = (p, size = 16) => `<span class="pchip" style="background:${cssColour(p.body, "var(--peach)")}">${glyph(p.icon, size)}</span>`;

