// ----- Preview animations -----
function flipPreview() {
  if (reduced) return;
  edPreview.animate([{ transform: "scaleY(1)" }, { transform: "scaleY(0.1)" }, { transform: "scaleY(1)" }], { duration: 200, easing: "steps(4, end)" });
}
function sparks() {
  if (reduced) return;
  const host = $("ed-preview-bg");
  const hr = host.getBoundingClientRect(), tr = edPreview.getBoundingClientRect();
  const cx = tr.left - hr.left + tr.width / 2, cy = tr.top - hr.top + tr.height / 2;
  const colors = ["sun", "mint", "sky", "coral", "lilac", "peach"];
  for (let i = 0; i < 10; i++) {
    const a = (i / 10) * Math.PI * 2;
    const sp = document.createElement("i");
    sp.className = "spark";
    sp.style.left = `${cx - 5}px`;
    sp.style.top = `${cy - 5}px`;
    sp.style.background = `var(--${colors[i % colors.length]})`;
    host.appendChild(sp);
    const dx = Math.round(Math.cos(a) * (tr.width / 2 + 20) / 4) * 4, dy = Math.round(Math.sin(a) * (tr.height / 2 + 20) / 4) * 4;
    sp.animate([{ transform: "translate(0, 0)", opacity: 1 }, { transform: `translate(${dx}px, ${dy}px)`, opacity: 0 }], { duration: 420, easing: "steps(5, end)" }).finished.then(() => sp.remove());
  }
}
// The preview plays the button's animation. A loop plays three times here so it does not run forever.
const previewCfg = () => ({ ...draft.anim, repeat: draft.anim.repeat === "loop" ? "three" : draft.anim.repeat });
function previewAnim() { if (draft) playAnim(edPreview, previewCfg(), false); }
function pressPreview() {
  playAnim(edPreview, previewCfg(), false);
  if (draft.states.length < 2) { $("ed-preview-note").textContent = "One look, so the button stays the same."; return; }
  stSel = (stSel + 1) % draft.states.length;
  closeIPick(false);
  renderStates();
  renderStateCard();
  // The flip would replace the press animation, so it only plays when there is none
  if (draft.anim.type === "none") flipPreview();
  refreshAll();
}
function stopPlay() {
  if (!playTimer) return;
  clearInterval(playTimer);
  playTimer = null;
  $("ed-play").setAttribute("aria-pressed", "false");
  $("ed-play").textContent = "Play states";
}
$("ed-try").addEventListener("click", pressPreview);
$("ed-play").addEventListener("click", () => {
  if (playTimer) { stopPlay(); return; }
  if (draft.states.length < 2) return;
  $("ed-play").setAttribute("aria-pressed", "true");
  $("ed-play").textContent = "Stop";
  pressPreview();
  playTimer = setInterval(pressPreview, reduced ? 1600 : 900);
});

