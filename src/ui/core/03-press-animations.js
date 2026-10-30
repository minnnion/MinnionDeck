// ===== Button press animations =====
// One push, like a key going down and back: the only press animation (the others were dropped,
// and a button that had one now pushes)
const ANIMS = [
  { id: "press", name: "Press", group: "Basic", dur: 160, desc: "Pushes in like a key" }
];
const ANIM = Object.fromEntries(ANIMS.map((x) => [x.id, x]));
const ANIM_GROUPS = [...new Set(ANIMS.map((x) => x.group))];
const SPEED = { slow: 1.6, normal: 1, fast: 0.6 };
const defaultAnim = () => ({ type: "press", speed: "normal", repeat: "once", phone: true });
function stopAnim(e) {
  if (!e) return;
  clearTimeout(e._animT);
  clearInterval(e._animI);
  [...e.classList].filter((c) => c === "anim" || c.startsWith("a-")).forEach((c) => e.classList.remove(c));
  e.classList.remove("pressed");
  delete e.dataset.loop;
}
// Every button pushes in once and back; "a" is kept for the callers but no longer chooses anything
function playAnim(e, a) {
  stopAnim(e);
  if (!e || !a || noMotion()) return;
  const dur = ANIM.press.dur;
  e.style.setProperty("--ad", `${dur}ms`);
  e.style.setProperty("--ai", "1");
  void e.offsetWidth;
  e.classList.add("anim", "a-press");
  e._animT = setTimeout(() => stopAnim(e), dur + 60);
}
// A real press
function pressAnim(e) {
  playAnim(e, { type: "press" });
}
function resumeAnim() {}

