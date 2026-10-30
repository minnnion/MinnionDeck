  // ===== Profiles =====
  const PROFILE_COLOURS = [["mint", "Mint"], ["sky", "Sky"], ["lilac", "Lilac"], ["peach", "Peach"], ["coral", "Coral"], ["sun", "Sun"]];
  function newProfile() {
    const key = uid("Board"); boards[key] = [];
    const used = new Set(PROFILES.map((p) => p.body));
    const body = ["peach", "sky", "lilac", "mint", "coral"].find((b) => !used.has(b)) || "peach";
    const p = { id: uid("pr-"), name: "New profile", body, icon: "star", auto: "Never", tree: [P(uid("p-"), "Main", key)] };
    PROFILES.push(p);
    A.freshCard = p.id;
    return p;
  }
  function addProfile() {
    const p = newProfile();
    renderProfiles();
    const input = $(`pname-${p.id}`);
    if (input) { input.focus(); input.select(); }
  }

  // A profile's colour: one tab stop, arrow keys move between colours, and the chosen colour is
  // named in words and ticked, so it never relies on colour alone
  function colourPick(p) {
    const box = el("div", { className: "pcolour" });
    box.dataset.pcol = p.id;
    const current = PROFILE_COLOURS.find(([v]) => v === p.body) || PROFILE_COLOURS[0];
    const capId = `pcol-${p.id}`;
    const cap = el("span", { className: "cap", id: capId, textContent: `Colour: ${current[1]}` });
    const row = el("div", { className: "swatch-pick" });
    row.setAttribute("role", "radiogroup");
    row.setAttribute("aria-labelledby", capId);
    const pick = (v) => {
      p.body = v;
      renderProfiles();
      const on = $("av-profiles").querySelector(`[data-pcol="${p.id}"] [aria-checked="true"]`);
      if (on) on.focus();
    };
    PROFILE_COLOURS.forEach(([v, label], i) => {
      const on = v === current[0];
      const sb = el("button", { type: "button" });
      sb.style.background = `var(--${v})`;
      sb.style.color = "var(--on-accent)";
      sb.setAttribute("role", "radio");
      sb.setAttribute("aria-checked", String(on));
      sb.setAttribute("aria-label", label);
      sb.tabIndex = on ? 0 : -1;
      if (on) sb.innerHTML = glyph("check", 16);
      sb.addEventListener("click", () => pick(v));
      sb.addEventListener("keydown", (e) => {
        const d = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[e.key];
        if (!d) return;
        e.preventDefault();
        pick(PROFILE_COLOURS[(i + d + PROFILE_COLOURS.length) % PROFILE_COLOURS.length][0]);
      });
      row.appendChild(sb);
    });
    box.append(cap, row);
    return box;
  }

  function duplicateProfile(p) {
    const copy = clone(p); copy.id = uid("pr-"); copy.name = `${p.name} copy`.slice(0, 24);
    walk(copy.tree, (n) => { n.id = uid(n.kind === "page" ? "p-" : "f-"); if (n.kind === "page") { const key = uid("Board"); boards[key] = clone(boards[n.board] || []).map((t) => ({ ...t, id: uid("t-") })); n.board = key; } });
    PROFILES.push(copy); A.freshCard = copy.id; renderProfiles(); say(`Duplicated ${p.name}`);
  }

  // Use this profile on its own line; Duplicate and Export side by side; Delete under them.
  // The only coral button is the one that confirms a delete.
  function profileActions(p) {
    const inUse = p.id === A.prof;
    const acts = el("div", { className: "pacts" });
    if (!inUse) acts.append(btn("Use this profile", "btn puse", () => { switchProfile(p.id); renderProfiles(); }));
    const more = el("div", { className: "pmore" });
    const del = btn("Delete", "btn sm pdel", () => { removing = p.id; renderProfiles(); });
    del.disabled = inUse || PROFILES.length === 1;
    more.append(btn("Duplicate", "btn sm", () => duplicateProfile(p)), btn("Export", "btn sm", () => exportProfile(p)), del);
    acts.append(more);
    if (del.disabled) acts.append(el("p", { className: "help", textContent: PROFILES.length === 1 ? "Keep at least one profile." : "In use. Switch to another profile to delete it." }));
    if (removing === p.id) {
      const conf = el("div", { className: "pconfirm" });
      conf.setAttribute("role", "alert");
      conf.append(el("span", { textContent: `Delete ${p.name}? Its pages and buttons go too.` }), el("div", { className: "row" }, [
        btn("Delete", "btn sm go", () => { PROFILES.splice(PROFILES.indexOf(p), 1); removing = null; renderProfiles(); say(`Deleted ${p.name}`); }),
        btn("Keep", "btn sm", () => { removing = null; renderProfiles(); })
      ]));
      acts.append(conf);
    }
    return acts;
  }

  function renderProfiles() {
    const box = $("av-profiles");
    box.innerHTML = "";
    const pad = el("div", { className: "vpad" });
    const head = el("div", { className: "vhead" });
    head.append(el("h3", { textContent: "Profiles" }));
    const sw = switchEl(SET.autoSwitch, "Switch profiles automatically", (v) => { SET.autoSwitch = v; renderProfiles(); renderStatus(); });
    const lab = el("span", { textContent: "Switch automatically" });
    head.append(lab, sw);
    const tools = el("div", { className: "row ptools" }, [btn("+ New profile", "btn", addProfile), btn("Import a profile…", "btn", pickAndImport)]);
    pad.append(head, el("p", { className: "muted", textContent: "A profile is a set of pages and folders. Minniondeck can switch to a profile when an app comes to the front, for example Streaming when OBS Studio opens." }), tools);
    const cards = el("div", { className: "pcards" });
    PROFILES.forEach((p) => {
      const c = el("div", { className: `pcard${p.id === A.prof ? " active" : ""}${p.id === A.freshCard && !reduced ? " fresh" : ""}` });
      const h = el("div", { className: "pcard-head" });
      h.innerHTML = chipFor(p, 18);
      const nf = el("div", { className: "field" });
      const nid = `pname-${p.id}`;
      const name = el("input", { id: nid, value: p.name, maxLength: 24 });
      name.addEventListener("input", () => { p.name = name.value; renderStatus(); });
      name.addEventListener("blur", () => { if (!name.value.trim()) { p.name = "Untitled"; name.value = p.name; } });
      nf.append(el("label", { htmlFor: nid, textContent: "Name" }), name);
      h.append(nf);
      const top = el("div", { className: "ptop" }, [h]);
      if (p.id === A.prof) top.append(el("span", { className: "chip", textContent: "In use" }));
      const pages = pagesOf(p);
      let folders = 0; walk(p.tree, (n) => { if (n.kind === "folder") folders++; });
      const btns = pages.reduce((n, { page }) => n + tilesOf(page).length, 0);
      const stats = el("p", { className: "stats", textContent: `${pages.length} ${pages.length === 1 ? "page" : "pages"}, ${folders} ${folders === 1 ? "folder" : "folders"}, ${btns} buttons` });
      const f = el("div", { className: "field" });
      const sid = `auto-${p.id}`;
      const l = el("label", { htmlFor: sid, textContent: "Switch to this profile when" });
      const s = el("select", { id: sid });
      s.disabled = !SET.autoSwitch;
      APPS.forEach((a) => s.appendChild(el("option", { value: a, textContent: a === "Never" ? "Never (switch by hand)" : `${a} is in front` })));
      s.value = p.auto || "Never";
      s.addEventListener("change", () => { p.auto = s.value; renderStatus(); });
      f.append(l, s);
      if (!SET.autoSwitch) f.append(el("span", { className: "help", textContent: "Turn on Switch automatically above to use this." }));
      const needs = pluginNeeds(p);
      c.append(top, stats, ...(needs ? [needs] : []), f, colourPick(p), profileActions(p));
      cards.appendChild(c);
    });
    A.freshCard = null;
    pad.append(cards, el("p", { className: "muted", textContent: "Profiles save automatically. Export one to share it as a .mdprofile file; double-click a .mdprofile file to add it here." }));
    box.appendChild(pad);
  }
