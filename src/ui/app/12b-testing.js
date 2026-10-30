  // ===== Testing =====
  // Things being tried out before they go on the deck. Speech: the app runs Gradium's Phonon model
  // (src-tauri/src/speech.rs) from the files of the Speech plugin (minnion_speech). The app is the
  // hub: any plugin can ask it to speak, and the list below shows who asked.
  UI_GLYPHS.flask = ["..####..", "...##...", "...##...", "..#..#..", ".#....#.", "#.####.#", "#.####.#", "########"];
  // Lines with numbers and short words, to hear how the model reads what callouts are made of
  const SPEECH_SAMPLES = ["100 knots", "V1. Rotate.", "Positive rate.", "Gear up.", "Climb to FL350.", "QNH 1013.", "Before start checklist complete."];
  const SPEECH = { status: null, voice: "default", text: "100 knots", runs: [] };
  const SPEECH_MAX_RUNS = 12;

  async function speechRefresh() {
    SPEECH.status = await backend.call("speech_status", {}, null);
    if (A.view === "testing") renderTesting(true);
  }
  const voiceName = (id) => {
    const v = SPEECH.status && SPEECH.status.voices.find((x) => x.id === id);
    return v ? v.name : id;
  };
  async function speechSay(text) {
    const line = String(text || "").trim();
    if (!line) { say("Write something to say first."); return; }
    try {
      const id = await backend.run("speech_say", { text: line, voice: SPEECH.voice });
      speechRun({ id, text: line, voice: SPEECH.voice, from: "Testing page", kind: "waiting" });
    } catch (e) { say(String(e)); }
  }
  // One line's progress, from the page or from a plugin (speech_event)
  function speechRun(e) {
    let r = SPEECH.runs.find((x) => x.id === e.id);
    if (!r) { r = { id: e.id }; SPEECH.runs.unshift(r); SPEECH.runs.length = Math.min(SPEECH.runs.length, SPEECH_MAX_RUNS); }
    // An event can come before speech_say's answer: never step back to "waiting"
    if (e.kind === "waiting" && r.kind) return;
    Object.assign(r, e);
    if (A.view === "testing") renderSpeechRuns();
  }
  backend.listen("speech_event", (e) => {
    speechRun(e);
    // The model loaded, or something failed: show it in the status line (only then, so the text
    // box isn't redrawn while typing)
    if (e.kind === "error" || (e.kind === "speaking" && !(SPEECH.status && SPEECH.status.loaded))) speechRefresh();
  });

  const RUN_WORDS = { waiting: "Waiting", loading: "Loading the model", speaking: "Speaking", done: "Spoken", skipped: "Stopped", error: "Didn't speak" };
  function renderSpeechRuns() {
    const list = $("sp-runs");
    if (!list) return;
    list.innerHTML = "";
    if (!SPEECH.runs.length) { list.appendChild(el("p", { className: "muted", textContent: "Nothing spoken yet. Lines asked for here or by a plugin show up here." })); return; }
    SPEECH.runs.forEach((r) => {
      const row = el("div", { className: `sp-run${r.kind === "error" ? " bad" : ""}` });
      const what = el("div", { className: "sp-what" });
      what.append(el("b", { textContent: r.text }), el("small", { textContent: `${voiceName(r.voice)} · asked by ${r.from} · ${RUN_WORDS[r.kind] || r.kind}` }));
      const nums = el("div", { className: "sp-nums" });
      if (r.synthMs != null && r.audioMs) {
        const speed = r.audioMs / Math.max(1, r.synthMs);
        nums.append(el("span", { textContent: `made in ${(r.synthMs / 1000).toFixed(2)} s` }), el("span", { textContent: `${(r.audioMs / 1000).toFixed(1)} s of speech` }), el("span", { textContent: `${speed.toFixed(1)}× real time` }));
      }
      if (r.error) nums.append(el("span", { textContent: r.error }));
      row.append(what, nums);
      list.appendChild(row);
    });
  }

  function speechVoiceSelect(voices) {
    const s = el("select", { id: "sp-voice" });
    const groups = [["en-us", "US English"], ["en-gb", "UK English"]];
    voices.filter((v) => v.id === "default").forEach((v) => s.appendChild(el("option", { value: v.id, textContent: "Default (the model's own)" })));
    groups.forEach(([locale, label]) => {
      const vs = voices.filter((v) => v.id !== "default" && v.locale === locale);
      if (!vs.length) return;
      const g = el("optgroup");
      g.label = label;
      vs.forEach((v) => g.appendChild(el("option", { value: v.id, textContent: `${v.name}${v.gender ? `, ${v.gender}` : ""}` })));
      s.appendChild(g);
    });
    if (!voices.some((v) => v.id === SPEECH.voice)) SPEECH.voice = voices[0] ? voices[0].id : "default";
    s.value = SPEECH.voice;
    s.addEventListener("change", () => { SPEECH.voice = s.value; renderSpeechAbout(); });
    return s;
  }
  // What the chosen voice is like, from the voice list
  function renderSpeechAbout() {
    const p = $("sp-about");
    if (!p) return;
    const v = SPEECH.status && SPEECH.status.voices.find((x) => x.id === SPEECH.voice);
    p.textContent = v ? v.description : "";
  }

  function speechPanel() {
    const panel = el("section", { className: "panel speech" });
    panel.setAttribute("aria-labelledby", "sp-h");
    panel.appendChild(el("h4", { id: "sp-h", textContent: "Speech" }));
    const st = SPEECH.status;
    const line = el("p", { className: "help", id: "sp-status" });
    if (!backend.available()) line.textContent = "Speech works in the Minniondeck desktop app.";
    else if (!st) line.textContent = "Asking the app about speech…";
    else if (!st.installed) line.textContent = "The speech model isn't installed. Install the Speech plugin (minnion_speech) in Settings › Plugins.";
    else {
      const loaded = st.loaded ? `Model loaded in ${st.loadMs} ms` : "The model loads with the first line";
      line.textContent = `${loaded} · ${st.voices.length} voices · Gradium Phonon 31M, on this PC`;
      if (st.error) line.textContent += ` · ${st.error}`;
    }
    panel.appendChild(line);
    if (st && st.installed) {
      const voiceRow = el("div", { className: "row" });
      voiceRow.append(el("label", { htmlFor: "sp-voice", textContent: "Voice" }), speechVoiceSelect(st.voices));
      panel.append(voiceRow, el("p", { className: "help", id: "sp-about" }));
      const ta = el("textarea", { id: "sp-text", rows: 3, maxLength: 500, value: SPEECH.text });
      ta.setAttribute("aria-label", "What to say");
      ta.addEventListener("input", () => { SPEECH.text = ta.value; });
      // Ctrl+Enter speaks, like sending a message
      ta.addEventListener("keydown", (e) => { if (e.key === "Enter" && e.ctrlKey) { e.preventDefault(); speechSay(SPEECH.text); } });
      const acts = el("div", { className: "row" });
      acts.append(btn("Speak", "btn go", () => speechSay(SPEECH.text)), btn("Stop", "btn", () => backend.call("speech_stop")));
      const samples = el("div", { className: "sp-samples" });
      samples.setAttribute("role", "group");
      samples.setAttribute("aria-label", "Sample lines");
      SPEECH_SAMPLES.forEach((t) => samples.appendChild(btn(t, "btn", () => { SPEECH.text = t; ta.value = t; speechSay(t); }, `Say ${t}`)));
      panel.append(ta, acts, el("b", { textContent: "Try these" }), samples);
    }
    panel.append(el("b", { textContent: "Last lines" }), el("div", { className: "sp-runs", id: "sp-runs" }));
    return panel;
  }

  // fromRefresh: drawn again because the engine's status came back (don't ask again)
  function renderTesting(fromRefresh) {
    const box = $("av-testing");
    const active = document.activeElement && box.contains(document.activeElement) ? document.activeElement.id : null;
    box.innerHTML = "";
    const pad = el("div", { className: "vpad" });
    const head = el("div", { className: "vhead" });
    head.append(el("h3", { textContent: "Testing" }));
    pad.append(head, el("p", { className: "help", textContent: "Try things out here before they go on the deck." }), speechPanel());
    box.appendChild(pad);
    renderSpeechAbout();
    renderSpeechRuns();
    if (active && $(active)) $(active).focus();
    if (!fromRefresh) speechRefresh();
  }
