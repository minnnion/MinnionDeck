// ===== Action catalogue =====
// The built-in actions. Installed plugins add theirs at start (addPluginAction below).
const ACTIONS = [
  { id: "key", label: "Press keys", group: "Keyboard and mouse", fields: [{ key: "keys", label: "Keys", kind: "keys", required: true, help: "Click the box, then press the keys together. Backspace clears it." }] },
  { id: "text", label: "Type text", group: "Keyboard and mouse", fields: [{ key: "text", label: "Text to type", kind: "text", required: true, max: 500 }] },
  { id: "click", label: "Mouse click", group: "Keyboard and mouse", fields: [{ key: "button", label: "Button", kind: "select", options: ["Left", "Right", "Middle"], required: true }] },
  { id: "run", label: "Open a program", group: "Apps and web", fields: [{ key: "path", label: "Program", kind: "text", required: true, drop: true, placeholder: "C:\\Program Files\\App\\app.exe", help: "Type its path, or drag the program or its shortcut here from your desktop or File Explorer." }, { key: "args", label: "Arguments", kind: "text", placeholder: "Optional" }] },
  { id: "url", label: "Open a website", group: "Apps and web", fields: [{ key: "url", label: "Address", kind: "url", required: true, placeholder: "https://" }] },
  { id: "vol", label: "Change volume", group: "Sound", fields: [{ key: "direction", label: "Direction", kind: "select", options: ["Up", "Down"], required: true }, { key: "amount", label: "Amount", kind: "number", min: 1, max: 100, unit: "%", def: 10, required: true }] },
  { id: "mic", label: "Mute microphone", group: "Sound", dual: ["Mic on", "Mic muted"], fields: [] },
  { id: "delay", label: "Wait", group: "Timing", fields: [{ key: "ms", label: "Wait for", kind: "number", min: 0, max: 60000, unit: "ms", def: 500, required: true }] },
  // Pages: the PC and phones turn to another page of the profile (a folder opens its first page),
  // or back to the page before. They run in Minniondeck itself, not on the PC (12-buttons.js; the
  // phone turns its own page, Deck.kt).
  { id: "page", label: "Go to page", group: "Pages", fields: [{ key: "page", label: "Page or folder", kind: "select", pages: true, required: true, help: "A folder opens its first page." }] },
  { id: "back", label: "Back", group: "Pages", fields: [] }
];
const ACTION = Object.fromEntries(ACTIONS.map((a) => [a.id, a]));
const ACTION_ICON = {
  key: "keyboard", text: "type", click: "mouse", run: "app", url: "globe", vol: "speaker", mic: "mic", delay: "hourglass", page: "folder", back: "up"
};
// Steps Minniondeck runs itself, turning pages
const NAV_STEPS = new Set(["page", "back"]);
const pluginFolder = (a) => a.pluginId ? `plugins/${a.pluginId}/` : `plugins/${a.plugin ? a.plugin.toLowerCase() : "builtin"}/icons/`;
const iconPath = (a) => a.pluginId ? `${pluginFolder(a)}${a.iconFile || "(no icon)"}` : `${pluginFolder(a)}${a.id}.png`;
// The icon chip takes its plugin's crew colour; built-in actions sit on the panel colour.
// An installed plugin's action shows the plugin's own picture.
function actionIcon(a) {
  if (a.iconUrl) return `<span class="act-ico img" aria-hidden="true"><img src="${a.iconUrl}" alt=""></span>`;
  const body = a.plugin && PLUGIN_BODY[a.plugin];
  const style = body ? ` style="background:var(--${body});color:var(--on-accent)"` : "";
  return `<span class="act-ico" aria-hidden="true"${style}>${glyph(ACTION_ICON[a.id] || "bolt", 16)}</span>`;
}
const GROUPS = [...new Set(ACTIONS.map((a) => a.group))];

// An installed plugin's action joins the catalogue. Its id, "plug:<plugin id>:<action id>", is also
// what the desktop app runs. Fields come from the plugin's actions.json, so only kinds the editor
// knows are kept, and a list without options becomes a text box.
const FIELD_KINDS = ["text", "number", "select", "url"];
function addPluginAction(p, def) {
  const fields = (Array.isArray(def.fields) ? def.fields : [])
    .filter((f) => f && typeof f.key === "string" && f.key)
    .map((f) => {
      const kind = FIELD_KINDS.includes(f.kind) ? f.kind : "text";
      const options = Array.isArray(f.options) ? f.options.map(String) : [];
      const out = { key: f.key, label: String(f.label || f.key), kind: kind === "select" && !options.length && !f.dynamic ? "text" : kind };
      if (out.kind === "select") out.options = options;
      // The plugin fills this list itself (see dynamicOptions in the editor)
      if (out.kind === "select" && f.dynamic) out.dynamic = true;
      if (f.def !== undefined) out.def = f.def;
      ["placeholder", "help", "unit"].forEach((k) => { if (typeof f[k] === "string") out[k] = f[k]; });
      ["min", "max"].forEach((k) => { if (typeof f[k] === "number") out[k] = f[k]; });
      if (f.required) out.required = true;
      return out;
    });
  const id = `plug:${p.id}:${def.id}`;
  const a = { id, label: String(def.label || def.id), group: def.group || p.name, plugin: p.name, pluginId: p.id, iconUrl: def.icon || "", iconFile: def.iconFile || "", fields };
  // Named states the plugin switches the button between (it does, not each press)
  const states = (Array.isArray(def.states) ? def.states : []).filter((s) => typeof s === "string" && s).slice(0, 16);
  if (states.length >= 2) { a.dual = states; a.ownStates = true; }
  // A knob: turned with the mouse wheel on the PC, a drag on a phone (the plugin hears "turn")
  if (def.knob) a.knob = true;
  // A screen (a map, a flight plan, a message log): it shows what the plugin sends, and a press
  // does nothing
  if (def.screen) a.screen = true;
  const i = ACTIONS.findIndex((x) => x.id === id);
  if (i >= 0) ACTIONS[i] = a; else ACTIONS.push(a);
  ACTION[id] = a;
  if (!GROUPS.includes(a.group)) GROUPS.push(a.group);
  return a;
}

// ===== Icon library =====
// The built-in icons. Plugin icons come with each plugin.
const ICON_SETS = [
  { name: "Built in", folder: "plugins/builtin/icons/", plugin: null, icons: ["play", "bolt", "note", "folder", "mic", "speaker", "keyboard", "type", "mouse", "app", "globe", "monitor", "hourglass", "star", "home", "lock", "bulb", "power", "gear", "check", "cross", "up", "down", "sun", "moon", "clapper", "broadcast", "rewind", "camera", "playpause", "skip", "heart", "chat", "flag", "headset"] }
];
const ICON_NAMES = {
  play: "Play", bolt: "Bolt", note: "Music note", folder: "Folder", mic: "Microphone", speaker: "Speaker", keyboard: "Keyboard",
  type: "Text", mouse: "Mouse", app: "Program", globe: "Website", monitor: "Monitor", hourglass: "Timer", star: "Star", home: "Home",
  lock: "Lock", bulb: "Light", power: "Power", gear: "Settings", check: "Check", cross: "Cross", up: "Arrow up", down: "Arrow down",
  sun: "Sun", moon: "Moon", clapper: "Scene", broadcast: "Go live", rewind: "Replay", camera: "Camera", playpause: "Play or pause",
  skip: "Next track", heart: "Like", chat: "Chat", flag: "Marker", headset: "Headset"
};
const iconCount = () => ICON_SETS.reduce((n, set) => n + set.icons.length, 0);
// Where an icon's file is: plugins/builtin/icons/<id>.png, or the file in its plugin's folder
const iconPathOf = (id) => (id.startsWith("plug:") ? `plugins/${id.split(":")[1]}/${id.split(":").slice(2).join(":")}` : `${iconSet(id).folder}${id}.png`);

// A plugin's icons become its own set in the icon picker, and its GIFs join every GIF list.
// Both are referred to as "plug:<plugin>:<file>"; buttons keep that id, and phones get the file
// (phones/sync.rs). Called for each plugin when the plugin list loads or changes.
function clearPluginMedia() {
  for (let i = ICON_SETS.length - 1; i >= 0; i--) if (ICON_SETS[i].pluginId) ICON_SETS.splice(i, 1);
  for (let i = GIFS.length - 1; i >= 0; i--) if (GIFS[i].pluginId) { delete GIF[GIFS[i].id]; GIFS.splice(i, 1); }
}
function setPluginMedia(p) {
  const icons = (p.icons || []).map((m) => {
    const id = `plug:${p.id}:${m.file}`;
    PLUGIN_ICON_URL[id] = m.url;
    ICON_NAMES[id] = m.name;
    return id;
  });
  if (icons.length) ICON_SETS.push({ name: p.name, folder: `plugins/${p.id}/`, plugin: p.name, pluginId: p.id, icons });
  (p.gifs || []).forEach((m) => {
    const g = { id: `plug:${p.id}:${m.file}`, name: m.name, desc: `From ${p.name}`, kind: "gif", group: p.name, file: true, src: m.url, w: 16, h: 16, n: 1, ms: 100, pluginId: p.id };
    // Its real size, for GIFs shown in the icon's place
    const img = new Image();
    img.onload = () => { g.w = img.naturalWidth || 16; g.h = img.naturalHeight || 16; };
    img.src = g.src;
    GIFS.push(g);
    GIF[g.id] = g;
  });
}
const iconSet = (id) => ICON_SETS.find((set) => set.icons.includes(id)) || ICON_SETS[0];
function iconChip(id) {
  const set = iconSet(id);
  const body = set.plugin && PLUGIN_BODY[set.plugin];
  const style = body ? ` style="background:var(--${body});color:var(--on-accent)"` : "";
  return `<span class="act-ico" aria-hidden="true"${style}>${glyph(id, 16)}</span>`;
}

