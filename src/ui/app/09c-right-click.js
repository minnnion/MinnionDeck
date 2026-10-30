  // ===== Right-click =====
  // The window's own browser menu (Back, Refresh, Print…) never shows: a right-click anywhere does
  // nothing, except in text boxes, which keep Cut, Copy and Paste.
  document.addEventListener("contextmenu", (e) => {
    if (e.target.closest("input:not([type=range]):not([type=color]), textarea, [contenteditable=true]")) return;
    e.preventDefault();
  });
