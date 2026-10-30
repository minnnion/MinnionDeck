// ----- The editor window -----
let edReturn = null, edClosing = 0;
function showEditor() {
  if (!draft) return;
  const m = $("ed-modal");
  // Opened again while it was still closing: stop the close
  clearTimeout(edClosing);
  m.classList.remove("closing");
  if (m.hidden) { edReturn = document.activeElement; m.hidden = false; }
  // The body and each of its two columns can scroll; every one starts at the top
  edForm.querySelectorAll(".ed-body, .ed-body .two, .ed-body .two > .stack").forEach((s) => { s.scrollTop = 0; });
  setTimeout(() => { edName.focus({ preventScroll: true }); previewAnim(); }, 60);
}
function hideEditor() {
  closePick(false); closeIPick(false); closeAPick(false); closeGPick(false); stopPlay();
  // The window shrinks and fades before it goes (at once when motion is off)
  const m = $("ed-modal");
  clearTimeout(edClosing);
  const gone = () => { m.classList.remove("closing"); m.hidden = true; };
  if (m.hidden || noMotion()) gone();
  else { m.classList.add("closing"); edClosing = setTimeout(gone, 140); }
  if (edReturn && edReturn.isConnected) edReturn.focus({ preventScroll: true });
}
$("ed-close").addEventListener("click", () => {
  if (!draft) { hideEditor(); return; }
  if (isNew) { removeTile(selectedId, false); say("New button discarded"); return; }
  const t = findTile(selectedId);
  const pickKeys = (o) => JSON.stringify(["label", "mode", "steps", "style", "states", "anim"].map((k) => o[k]));
  const changed = pickKeys(draft) !== pickKeys(t);
  hideEditor();
  say(changed ? "Editor closed. Your changes were not saved." : "Editor closed");
});
$("ed-dialog").addEventListener("keydown", (e) => {
  if (e.key === "Escape") { e.preventDefault(); $("ed-close").click(); return; }
  if (e.key !== "Tab") return;
  const f = [...$("ed-dialog").querySelectorAll("button, input, select, [tabindex='0']")].filter((x) => !x.disabled && x.offsetParent !== null);
  if (!f.length) return;
  if (e.shiftKey && document.activeElement === f[0]) { e.preventDefault(); f[f.length - 1].focus(); }
  else if (!e.shiftKey && document.activeElement === f[f.length - 1]) { e.preventDefault(); f[0].focus(); }
});
$("ed-open-demo").addEventListener("click", () => {
  const t = findTile("live") || boards.Main[0];
  if (!t) return;
  openEditor(t.id, false);
  showEditor();
});

