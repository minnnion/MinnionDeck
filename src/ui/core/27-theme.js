// ===== Theme =====
const themeBtn = $("theme-toggle"), sysDark = window.matchMedia("(prefers-color-scheme: dark)");
let savedTheme = null;
try { savedTheme = localStorage.getItem("minniondeck-theme"); } catch (e) {}
const isDark = () => {
  const t = document.documentElement.dataset.theme;
  return t ? t === "dark" : sysDark.matches;
};
function syncTheme() {
  const dark = isDark();
  themeBtn.setAttribute("aria-pressed", String(dark));
  $("theme-icon").innerHTML = glyph(dark ? "moon" : "sun", 20);
  renderSwatches();
}
if (savedTheme === "dark" || savedTheme === "light") document.documentElement.dataset.theme = savedTheme;
themeBtn.addEventListener("click", () => {
  const next = isDark() ? "light" : "dark";
  document.documentElement.dataset.theme = next;
  try { localStorage.setItem("minniondeck-theme", next); } catch (e) {}
  syncTheme();
  say(next === "dark" ? "Night shift. The crew turned the lights down." : "Day shift. Lights back on.");
});
sysDark.addEventListener("change", () => { if (!document.documentElement.dataset.theme) syncTheme(); });
syncTheme();

