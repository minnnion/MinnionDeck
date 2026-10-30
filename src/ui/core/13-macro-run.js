// ===== Macro run =====
const track = $("track"), macroStatus = $("macro-status"), runBtn = $("run-macro");
let macroBusy = false;
let walkerEl = null, walkerX = 0;
function walkerDraw(expr = "open", feet = "idle", lk = 0) {
  walkerEl.innerHTML = minnion({ c: pal("mint"), expr, feet, look: lk, scale: 3 });
}
const posFor = (pip) => pip.offsetLeft + pip.offsetWidth / 2 - 24;
function renderMacro(t) {
  if (!t) {
    $("macro-title").textContent = "Go live was deleted";
    track.innerHTML = "";
    runBtn.disabled = true;
    return;
  }
  $("macro-title").textContent = `${t.label}, ${t.steps.length} ${t.steps.length === 1 ? "step" : "steps"}`;
  runBtn.textContent = `Run ${t.label}`;
  runBtn.disabled = false;
  track.style.gridTemplateColumns = `repeat(${t.steps.length}, minmax(0, 1fr))`;
  track.innerHTML = "";
  t.steps.forEach((s, i) => {
    const pip = document.createElement("div");
    pip.className = "pip";
    pip.innerHTML = `<span><span class="mark"></span> <span class="name"></span></span><span class="val"></span>`;
    pip.querySelector(".mark").textContent = `${i + 1}.`;
    pip.querySelector(".name").textContent = ACTION[s.type].label;
    pip.querySelector(".val").textContent = summary(s);
    track.appendChild(pip);
  });
  walkerEl = document.createElement("div");
  walkerEl.className = "walker";
  track.appendChild(walkerEl);
  walkerX = posFor(track.querySelector(".pip"));
  walkerEl.style.transform = `translateX(${walkerX}px)`;
  walkerDraw();
}
async function walkTo(x) {
  if (reduced || x === walkerX) { walkerEl.style.transform = `translateX(${x}px)`; walkerX = x; return; }
  let s = 0;
  const lk = x > walkerX ? 1 : -1;
  const steps = setInterval(() => walkerDraw("open", s++ % 2 ? "a" : "b", lk), 90);
  await walkerEl.animate([{ transform: `translateX(${walkerX}px)` }, { transform: `translateX(${x}px)` }], { duration: 360, easing: "steps(6, end)", fill: "forwards" }).finished;
  clearInterval(steps);
  walkerEl.style.transform = `translateX(${x}px)`;
  walkerX = x;
  walkerDraw("open", "idle", lk);
}
async function runMacro(t) {
  if (macroBusy || !t) return;
  macroBusy = true;
  runBtn.disabled = true;
  renderMacro(t);
  runBtn.disabled = true;
  const pips = [...track.querySelectorAll(".pip")];
  try {
    for (let i = 0; i < t.steps.length; i++) {
      await walkTo(posFor(pips[i]));
      pips[i].classList.add("now");
      macroStatus.textContent = `Step ${i + 1} of ${t.steps.length}: ${ACTION[t.steps[i].type].label}`;
      await wait(300);
      const owner = pluginOf(t.steps[i]);
      const st = owner ? pluginState(owner) : "ready";
      if (st !== "ready") {
        pips[i].classList.replace("now", "fail");
        pips[i].querySelector(".mark").textContent = "✗";
        walkerDraw("dizzy");
        const fix = st === "dizzy" ? `Restart it in Crew status, then run again.` : st === "waking" ? "Wait a moment, then run again." : "Hire it below, then run again.";
        const msg = `Stopped at step ${i + 1}. ${owner} is ${STATE_WORDS[st]}. ${fix}`;
        macroStatus.textContent = msg;
        say(msg);
        return;
      }
      pips[i].classList.replace("now", "done");
      pips[i].querySelector(".mark").textContent = "✓";
    }
    walkerDraw("happy");
    macroStatus.textContent = `All ${t.steps.length} steps done.`;
  } finally {
    macroBusy = false;
    runBtn.disabled = false;
  }
}
runBtn.addEventListener("click", () => runMacro(findTile("live")));

