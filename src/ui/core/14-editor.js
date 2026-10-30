// ===== Editor =====
// Rebuilt from the original CreateEditButtonModal: preview and look on the left,
// name, action picker and the action's own fields on the right. Sequences are edited in place.
let draft = null;      // working copy of the tile being edited
let ownBefore = [];    // the button's own designs when the editor opened
let isNew = false;
let sel = 0;           // selected step
let stSel = 0;         // which state the look controls edit, and the preview shows
let justAdded = -1;    // state chip to animate in
let freshStep = -1, movedStep = -1; // step rows to animate
let playTimer = null;  // Play states loop
let showAll = false;   // show every error after a save attempt
let confirmDelete = false;
let searchText = "";
const touched = new Set();

const edForm = $("ed-form"), edName = $("ed-name"), edPreview = $("ed-preview");

function openEditor(id, fresh) {
  selectedId = id;
  isNew = fresh;
  const t = findTile(id);
  draft = clone(t);
  // This button's own designs as they were, so Cancel can put back what Adjust changed
  ownBefore = t.states.map((st) => designById(st.design)).filter((d) => d && d.own).map((d) => clone(d));
  // A background GIF now lives in the GIF section (played behind the icon), so the Image section
  // only holds pictures and designs. Saving the button keeps it that way.
  draft.states.forEach((st) => {
    if (st.bgType !== "gif") return;
    if (st.bgGif && !gifOf(st)) st.gif = { id: st.bgGif.id, place: "bg", speed: st.bgGif.speed || "normal" };
    st.bgType = "colour";
  });
  sel = 0;
  stSel = Math.min(t.cur || 0, t.states.length - 1);
  stopPlay();
  closeIPick(false);
  showAll = false;
  confirmDelete = false;
  searchText = "";
  touched.clear();
  render();
  renderEditor();
}

function validate(d) {
  const probs = [];
  if (!d.label.trim()) probs.push({ where: "name", step: -1, msg: "Give the button a name." });
  d.steps.forEach((s, i) => {
    const pre = d.mode === "sequence" ? `Step ${i + 1}: ` : "";
    ACTION[s.type].fields.forEach((fd) => {
      const v = String(s.f[fd.key] ?? "").trim();
      let m = null;
      if (fd.required && v === "") m = `${fd.label} is empty.`;
      else if (fd.kind === "url" && v && !/^https?:\/\/[^\s.]+\.[^\s]+$/.test(v)) m = "Address must start with http:// or https://.";
      else if (fd.kind === "number" && v !== "") {
        const n = Number(v);
        if (!Number.isFinite(n) || n < fd.min || n > fd.max) m = `${fd.label} must be between ${fd.min} and ${fd.max}.`;
      }
      if (m) probs.push({ where: `${i}:${fd.key}`, step: i, msg: pre + m, short: m });
    });
  });
  return probs;
}
function pluginWarnings(d) {
  const out = [];
  d.steps.forEach((s, i) => {
    const p = pluginOf(s);
    if (p && pluginState(p) !== "ready" && !out.some((w) => w.plugin === p)) out.push({ plugin: p, state: pluginState(p), step: i });
  });
  return out;
}

function segment(el, options, value, onPick, disabled = () => false) {
  el.innerHTML = "";
  options.forEach(([v, label]) => {
    const b = document.createElement("button");
    b.type = "button";
    b.textContent = label;
    b.setAttribute("aria-pressed", String(v === value));
    b.disabled = disabled(v);
    b.addEventListener("click", () => onPick(v));
    el.appendChild(b);
  });
}
