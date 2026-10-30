// ===== Crew status =====
const crewEl = $("crew");
const STATE_TEXT = { ready: "Ready", waking: "Starting up", dizzy: "Stopped" };
function crewChanged() {
  renderCrew();
  if (draft) { renderPicker(); renderWarn(); refreshStatus(); }
}
function renderCrew() {
  crewEl.innerHTML = "";
  plugins.forEach((p) => {
    const d = document.createElement("div");
    d.className = `member ${p.state}`;
    const expr = p.state === "dizzy" ? "dizzy" : p.state === "waking" ? "sleepy" : "open";
    const c = p.state === "dizzy" ? pal("coral", "var(--sun)") : pal(PLUGIN_BODY[p.name]);
    d.innerHTML = `${minnion({ c, expr, scale: 3 })}${p.state === "waking" ? '<span class="zzz" aria-hidden="true">z</span>' : ""}<div class="stack" style="gap:4px"><b></b><span class="state"></span></div>`;
    d.querySelector("b").textContent = p.name;
    d.querySelector(".state").textContent = STATE_TEXT[p.state];
    crewEl.appendChild(d);
  });
}

// ===== Phone pairing =====
$("phone-sprite").innerHTML = minnion({ c: pal("sky"), expr: "happy", scale: 4 });
const codeEl = $("code"), timerEl = $("timer"), countdown = $("countdown");
let expiresAt = 0;
function newCode() {
  codeEl.textContent = String(Math.floor(100000 + Math.random() * 900000)).replace(/(\d{3})(\d{3})/, "$1 $2");
  expiresAt = Date.now() + 300000;
  timerEl.style.animation = "none";
  void timerEl.offsetWidth;
  timerEl.style.animation = "";
  tick();
}
function tick() {
  const left = Math.max(0, Math.round((expiresAt - Date.now()) / 1000));
  countdown.textContent = `${Math.floor(left / 60)}:${String(left % 60).padStart(2, "0")}`;
}
setInterval(tick, 1000);
$("new-code").addEventListener("click", newCode);
newCode();

// ===== Empty state, blink =====
const emptySprite = $("empty-sprite");
let waveUp = true;
const drawWave = () => { emptySprite.innerHTML = minnion({ c: pal("sky"), expr: "happy", arm: waveUp, scale: 5 }); };
drawWave();
setInterval(() => { if (reduced) return; waveUp = !waveUp; drawWave(); }, 400);
setInterval(() => {
  if (busy || reduced) return;
  const i = Math.floor(Math.random() * crew.length);
  crew[i].expr = "sleepy";
  renderSeat(i);
  setTimeout(() => { if (!busy) { crew[i].expr = "open"; renderSeat(i); } }, 150);
}, 2200);

// ===== Design notes =====
const STATES = [
  [{ c: pal("mint"), expr: "open" }, "Ready, watching the board", "Bench, ready plugins, tile corners"],
  [{ c: pal("mint"), expr: "surprised", pose: "squash" }, "Getting ready to run", "First frame of every run"],
  [{ c: pal("sky"), expr: "happy", pose: "stretch" }, "Job done", "Hop on arrival, bench cheer, editor ready to save"],
  [{ c: pal("sky"), expr: "surprised" }, "Something is missing", "Editor, next to the first problem"],
  [{ c: pal("lilac"), expr: "surprised", look: -1, tall: true }, "Something just started", "Bench mates when a runner leaves"],
  [{ c: pal("sky"), expr: "sleepy" }, "Blinking, or a plugin starting up", "Bench, starting plugins"],
  [{ c: pal("coral", "var(--sun)"), expr: "dizzy" }, "Plugin stopped, or a step failed", "Crew status, Macro run, editor warning"],
  [{ c: pal("peach"), expr: "open", arm: true }, "Hello, or asking to join", "Empty board, Hire a crew member"]
];
const tb = $("states");
STATES.forEach(([opts, means, where]) => {
  const tr = document.createElement("tr");
  tr.innerHTML = `<td>${minnion({ ...opts, scale: 3 })}</td><td></td><td></td>`;
  tr.children[1].textContent = means;
  tr.children[2].textContent = where;
  tb.appendChild(tr);
});
const TOKENS = [
  ["--ink", "Outlines and text"], ["--panel", "Panels"], ["--tile", "Tiles"],
  ["--mint", "On, done, crew body"], ["--coral-dark", "Main action, failure, errors"], ["--coral", "Resize handle, dizzy body"],
  ["--sky", "Crew body"], ["--sky-dark", "Focus ring"], ["--lilac", "Crew body"],
  ["--peach", "Crew body"], ["--sun", "Dizzy antenna only"], ["--slate", "Secondary text"],
  ["--ground", "Page background"], ["--shadow", "Hard shadows"], ["--eye", "Pupils and mouths"],
  ["--focus", "Focus ring"], ["--coral-tint", "Error field fill"]
];
const sw = $("swatches");
function renderSwatches() {
  sw.innerHTML = "";
  TOKENS.forEach(([tok, use]) => {
    const hex = getComputedStyle(document.documentElement).getPropertyValue(tok).trim();
    const d = document.createElement("div");
    d.className = "swatch";
    d.innerHTML = `<div class="chipc" style="background:var(${tok})"></div><div class="meta"><b></b><br><span class="muted"></span></div>`;
    d.querySelector("b").textContent = `${tok.slice(2)} ${hex}`;
    d.querySelector(".muted").textContent = use;
    sw.appendChild(d);
  });
}
renderSwatches();

