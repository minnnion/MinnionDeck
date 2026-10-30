// ===== Logo animations for design layers =====
// Entrances play once, or back and forth when looped. Loops keep moving.
const LANIMS = [
  { id: "none", name: "None", kind: "in", dur: 0 },
  { id: "fade", name: "Fade in", kind: "in", dur: 700 }, { id: "slideup", name: "Slide up", kind: "in", dur: 700 }, { id: "slidein", name: "Slide in", kind: "in", dur: 700 },
  { id: "pop", name: "Pop in", kind: "in", dur: 600 }, { id: "drop", name: "Drop", kind: "in", dur: 800 }, { id: "zoom", name: "Zoom in", kind: "in", dur: 700 },
  { id: "type", name: "Typewriter", kind: "in", dur: 900, text: true },
  { id: "spin", name: "Spin", kind: "loop", dur: 1600 }, { id: "bounce", name: "Bounce", kind: "loop", dur: 900 }, { id: "float", name: "Float", kind: "loop", dur: 1800 },
  { id: "pulse", name: "Pulse", kind: "loop", dur: 1000 }, { id: "wiggle", name: "Wiggle", kind: "loop", dur: 700 }, { id: "blink", name: "Blink", kind: "loop", dur: 1000 }
];
const LANIM = Object.fromEntries(LANIMS.map((a) => [a.id, a]));
function wrapAnim(l, inner) {
  const a = l.la && LANIM[l.la.type];
  if (!a || a.id === "none") return `<span class="dla">${inner}</span>`;
  const n = a.kind === "loop" || l.la.loop ? "infinite" : "1";
  const dir = a.kind === "in" && l.la.loop ? "alternate" : "normal";
  const steps = a.id === "type" ? Math.max(1, String(l.text || "").length) : a.id === "spin" ? 12 : 8;
  // la.type/speed are looked up above (a.id, SPEED[...]), so they're already constrained; delay is
  // coerced to a number so an imported design can't inject into this style attribute.
  const delay = Math.max(0, Number(l.la.delay) || 0);
  return `<span class="dla la-${a.id}" style="animation-duration:${Math.round(a.dur * (SPEED[l.la.speed] || 1))}ms;animation-delay:${delay}ms;animation-iteration-count:${n};animation-direction:${dir};animation-timing-function:steps(${steps}, end)">${inner}</span>`;
}

