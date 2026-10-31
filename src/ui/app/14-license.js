  // ===== Activation =====
  // The app runs buttons only when it's licensed (src-tauri/src/license.rs gates run_button too).
  // When it isn't, this covers the window with an activation screen: the PC's machine code to send,
  // and a box for the licence key. In the browser mockup (no backend) the status is "licensed", so
  // this never shows there.
  async function licenseGate() {
    const st = await backend.call("license_status", {}, { state: "licensed", name: "browser" });
    if (!st || st.state === "licensed") return;
    backend.showWindow();
    const code = await backend.call("license_machine_code", {}, "—");

    const esc = (s) => String(s == null ? "" : s).replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" }[c]));
    const wrap = document.createElement("div");
    wrap.className = "lic-gate";
    wrap.innerHTML = `
      <div class="lic-card">
        <div class="lic-logo" aria-hidden="true"></div>
        <h1>Activate Minniondeck</h1>
        <p class="lic-lead">This copy isn't activated yet. Enter your licence key below to use the buttons.</p>
        <label class="lic-cap" for="lic-mc">Your machine code</label>
        <div class="lic-row">
          <input id="lic-mc" class="lic-mono" readonly value="${esc(code)}">
          <button type="button" class="lic-btn" id="lic-copy">Copy</button>
        </div>
        <p class="lic-hint">Send this code to the Minniondeck owner to get a key for this PC.</p>
        <label class="lic-cap" for="lic-key">Licence key</label>
        <textarea id="lic-key" class="lic-mono" rows="4" placeholder="Paste your licence key (or open your .mdlicense file and paste its text)"></textarea>
        <div class="lic-row lic-actions">
          <button type="button" class="lic-btn go" id="lic-go">Activate</button>
          <span class="lic-msg" id="lic-msg" aria-live="polite"></span>
        </div>
      </div>`;
    document.body.appendChild(wrap);

    const logo = wrap.querySelector(".lic-logo");
    if (typeof appIconSVG === "function") logo.innerHTML = appIconSVG(40);
    const msg = wrap.querySelector("#lic-msg");
    const setMsg = (t, bad) => { msg.textContent = t; msg.classList.toggle("bad", !!bad); };

    if (st.state === "wrongmachine") setMsg("The licence on this PC was made for a different machine. Get one for this PC's code.", true);
    else if (st.state === "bad") setMsg(`The stored licence is ${esc(st.why || "not valid")}.`, true);

    wrap.querySelector("#lic-copy").addEventListener("click", async () => {
      try { await navigator.clipboard.writeText(code); setMsg("Copied the machine code."); }
      catch { wrap.querySelector("#lic-mc").select(); setMsg("Press Ctrl+C to copy."); }
    });

    const go = wrap.querySelector("#lic-go");
    go.addEventListener("click", async () => {
      const key = wrap.querySelector("#lic-key").value.trim();
      if (!key) { setMsg("Paste your licence key first.", true); return; }
      go.disabled = true; setMsg("Checking…");
      const r = await backend.call("license_activate", { key }, { state: "bad", why: "no backend" });
      if (r && r.state === "licensed") { setMsg("Activated. Starting Minniondeck…"); setTimeout(() => location.reload(), 700); return; }
      go.disabled = false;
      if (r && r.state === "wrongmachine") setMsg("That key is for a different PC. Check the machine code matches this one.", true);
      else setMsg(`That didn't work: ${esc((r && r.why) || "the key isn't valid")}.`, true);
    });
  }

  const LIC_CSS = `
    .lic-gate { position: fixed; inset: 0; z-index: 9999; display: grid; place-items: center; padding: 24px;
      background: #04060b; background-image: radial-gradient(#1c2232 2px, transparent 2px); background-size: 16px 16px;
      font-family: "Pixelify Sans", ui-monospace, Consolas, monospace; color: #e4eaf6; animation: lic-in .2s steps(4, end); }
    @keyframes lic-in { from { opacity: 0; } }
    .lic-card { width: min(520px, 100%); background: #1c2232; border: 4px solid #000; box-shadow: 8px 8px 0 #04060b; padding: 24px 26px; }
    .lic-logo { display: grid; place-items: center; width: 56px; height: 56px; margin: 0 auto 6px; background: #262e43; border: 4px solid #000; image-rendering: pixelated; }
    .lic-card h1 { margin: 6px 0 4px; font-size: 24px; text-align: center; }
    .lic-lead { margin: 0 0 18px; text-align: center; color: #a4b0c8; font-size: 15px; }
    .lic-cap { display: block; margin: 12px 0 5px; font-family: "IBM Plex Mono", ui-monospace, monospace; font-size: 11px; letter-spacing: .08em; text-transform: uppercase; color: #a4b0c8; }
    .lic-mono { font-family: "IBM Plex Mono", ui-monospace, Consolas, monospace; }
    .lic-row { display: flex; gap: 8px; align-items: center; }
    .lic-card input, .lic-card textarea { flex: 1; min-width: 0; background: #131826; color: #e4eaf6; border: 3px solid #3a4560; padding: 8px 10px; font-size: 14px; resize: vertical; }
    .lic-card input:focus, .lic-card textarea:focus, .lic-btn:focus-visible { outline: 3px solid #ffc93c; outline-offset: 1px; }
    .lic-btn { flex: none; font: inherit; font-size: 15px; color: #e4eaf6; background: #262e43; border: 3px solid #e4eaf6; padding: 6px 14px; cursor: pointer; box-shadow: 3px 3px 0 #000; }
    .lic-btn:hover { background: #303954; }
    .lic-btn:active { transform: translate(3px, 3px); box-shadow: none; }
    .lic-btn.go { background: #5fd0a8; color: #1e2433; border-color: #000; font-weight: 700; }
    .lic-btn.go:hover { background: #7fe0bd; }
    .lic-btn:disabled { opacity: .55; cursor: default; }
    .lic-hint { margin: 6px 0 0; font-size: 13px; color: #a4b0c8; }
    .lic-actions { margin-top: 16px; }
    .lic-msg { font-size: 14px; color: #a4b0c8; }
    .lic-msg.bad { color: #ff8a8e; }`;
  const licStyle = document.createElement("style");
  licStyle.textContent = LIC_CSS;
  document.head.appendChild(licStyle);

  licenseGate();
