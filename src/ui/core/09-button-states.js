// ===== Button states =====
// Every button has one or more states. Each press moves it to the next state and wraps around.
const MAX_STATES = 6;
function normalizeTile(t) {
  const dual = t.mode === "single" && ACTION[t.steps[0].type].dual;
  const base = { text: "", glyph: t.glyph, gif: GIF_NONE(), bgType: "colour" };
  t.states = dual
    ? [{ ...base, name: dual[0], bg: t.style.bg }, { ...base, name: dual[1], bg: t.style.onBg }]
    : [{ ...base, name: "Default", bg: t.style.bg }];
  t.cur = dual && t.on ? 1 : 0;
  t.anim = defaultAnim();
  delete t.glyph; delete t.on; delete t.style.bg; delete t.style.onBg;
  return t;
}
function newStep(id) {
  const a = ACTION[id];
  const f = {};
  a.fields.forEach((fd) => { f[fd.key] = fd.kind === "select" && fd.def === undefined ? (fd.options[0] ?? "") : fd.def !== undefined ? String(fd.def) : ""; });
  return { type: id, f };
}
function summary(s) {
  const f = s.f || {};
  switch (s.type) {
    case "key": return f.keys || "No keys yet";
    case "text": return f.text ? `"${f.text}"` : "No text yet";
    case "click": return `${f.button} click`;
    case "run": return f.path || "No program yet";
    case "url": return f.url || "No address yet";
    case "vol": return `${f.direction} ${f.amount}%`;
    case "delay": return `${f.ms} ms`;
    default: return ACTION[s.type].label;
  }
}

