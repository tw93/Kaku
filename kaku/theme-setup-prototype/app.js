// THROWAWAY PROTOTYPE: three variants of the theme setup UI, selected by ?variant=.
const variants = {
  A: "Inspector",
  B: "Guided setup",
  C: "Status workbench",
};

const tools = [
  { id: "fish", name: "Fish", status: "safe", state: "Safe to install", timing: "Live", checked: true, note: "4.3.3 · native dual theme", selector: "No existing theme selection", targets: ["~/.config/fish/themes/Kaku.theme", "~/.config/fish/conf.d/kaku-theme.fish"], scope: "Creates two Kaku-owned files. Does not edit universal variables." },
  { id: "fzf", name: "fzf", status: "safe", state: "Safe to install", timing: "Next launch", checked: true, note: "0.60.3 · no custom colors", selector: "FZF_DEFAULT_OPTS_FILE is absent", targets: ["~/.config/kaku/themes/fzf-dark.opts", "~/.config/kaku/themes/fzf-light.opts", "Kaku shell startup block"], scope: "Adds a Kaku-owned options pointer. Existing FZF_DEFAULT_OPTS remains untouched." },
  { id: "claude", name: "Claude Code", status: "takeover", state: "Decision required", timing: "Live / restart once", checked: false, note: "2.1.119 · current theme: auto", selector: "theme = auto in ~/.claude/settings.json", targets: ["~/.claude/themes/kaku-light.json", "~/.claude/themes/kaku-dark.json", "~/.claude/settings.json → theme"], scope: "Manages only the user-level theme selector and two Kaku-owned theme files." },
  { id: "codex", name: "Codex", status: "info", state: "ANSI inheritance", timing: "Terminal palette", checked: false, note: "0.147.0 · follows Kaku terminal colors without config writes", selector: "No Kaku selector; user tui.theme remains untouched", targets: ["Kaku terminal ANSI palette", "COLORTERM=truecolor / COLORFGBG"], scope: "Informational only. Theme Setup never edits $CODEX_HOME/config.toml; native Codex theme support is deferred." },
  { id: "opencode", name: "OpenCode", status: "shadowed", state: "Shadowed", timing: "Next launch", checked: false, note: "1.2.7 · project theme overrides user config", selector: "Project .opencode/tui.json has higher precedence", targets: ["~/.config/opencode/themes/kaku.json", "~/.config/opencode/tui.json → theme"], scope: "Can install user-level Kaku theme, but the current project will continue to override it." },
  { id: "starship", name: "Starship", status: "takeover", state: "Custom palette", timing: "Next prompt", checked: false, note: "1.23.0 · palette = catppuccin", selector: "palette = catppuccin in ~/.config/starship.toml", targets: ["~/.config/starship.toml → palettes.kaku_*", "~/.config/starship.toml → palette"], scope: "Adds two Kaku palette tables and takes over only the root palette selector." },
  { id: "btop", name: "btop", status: "drift", state: "Modified outside Kaku", timing: "Suspended", checked: false, note: "1.4.5 · managed selector changed to gruvbox", selector: "Expected color_theme = Kaku; found gruvbox", targets: ["~/.config/btop/btop.conf → color_theme", "~/.config/btop/themes/Kaku.theme"], scope: "Sync is paused. Resolve drift before Kaku can update or remove this adapter." },
];

const state = {
  selected: "fish",
  checked: new Set(tools.filter(t => t.checked).map(t => t.id)),
  step: 0,
  takeover: "keep",
  results: false,
};

function variant() {
  const v = new URLSearchParams(location.search).get("variant")?.toUpperCase();
  return variants[v] ? v : "A";
}

function setVariant(v) {
  const url = new URL(location.href);
  url.searchParams.set("variant", v);
  history.replaceState({}, "", url);
  render();
}

function statusClass(tool) {
  return tool.status === "safe" ? "ok" : tool.status === "drift" ? "bad" : tool.status === "shadowed" ? "info" : "warn";
}

function status(tool) {
  return `<span class="status ${statusClass(tool)}"><span class="dot"></span>${tool.state}</span>`;
}

function renderTop(title, copy) {
  return `<header class="topbar"><div><div class="brand">Kaku <span>· Theme coordination</span></div><h1>${title}</h1><p class="lede">${copy}</p></div><div class="mode-chip">Current appearance <strong>Dark</strong> · Auto</div></header>`;
}

function toggle(id) {
  if (state.checked.has(id)) state.checked.delete(id); else state.checked.add(id);
  state.selected = id;
  render();
}

function renderA() {
  const selected = tools.find(t => t.id === state.selected) || tools[0];
  return `<main class="shell">
    ${renderTop("Choose which tools Kaku may coordinate", "Installed tools are detected locally. Safe changes are preselected; existing choices always require you to opt in.")}
    <section class="inspector">
      <div class="panel tool-table">
        <div class="table-head"><span></span><span>Tool</span><span>Status</span><span>Activation</span></div>
        ${tools.map(t => `<div class="tool-row ${t.id === selected.id ? "selected" : ""}" data-select="${t.id}">
          <input type="checkbox" data-toggle="${t.id}" ${state.checked.has(t.id) ? "checked" : ""} ${t.status === "drift" ? "disabled" : ""}/>
          <div><div class="tool-name">${t.name}</div><div class="tool-note">${t.note}</div></div>
          ${status(t)}<span class="tag">${t.timing}</span>
        </div>`).join("")}
      </div>
      <aside class="panel details">
        <div class="status ${statusClass(selected)}"><span class="dot"></span>${selected.state}</div>
        <h2 style="margin-top:10px">${selected.name}</h2>
        <dl><dt>Detected selector</dt><dd>${selected.selector}</dd><dt>Activation</dt><dd>${selected.timing}</dd><dt>Targets</dt><dd>${selected.targets.join("<br>")}</dd></dl>
        <div class="scope"><strong>Authorization scope</strong><br>${selected.scope}</div>
        ${selected.status === "takeover" ? `<div class="footer-actions"><button class="button" data-action="preview-takeover">Review takeover</button></div>` : ""}
        ${selected.status === "drift" ? `<div class="footer-actions"><button class="button" data-action="resolve">Resolve drift</button></div>` : ""}
      </aside>
    </section>
    <div class="footer-actions"><button class="button">Cancel</button><button class="button primary" data-action="preview">Review ${state.checked.size} changes</button></div>
  </main>`;
}

function stepBody() {
  if (state.results) return `<h2>Theme coordination finished</h2><p class="lede">Each tool was applied as its own transaction.</p><div class="preview-list">
    <div class="preview-item">✓ Fish <span class="subtle">changed · live</span></div>
    <div class="preview-item">• Codex <span class="subtle">informational · inherited ANSI</span></div>
    <div class="preview-item">! fzf <span style="color:var(--red)">failed, rolled back</span><div class="preview-path">Shell startup file changed after preview. Nothing was left modified.</div></div>
  </div>`;
  if (state.step === 0) return `<h2>7 supported tools found</h2><p class="lede">Kaku checked versions, theme selectors, configuration precedence, and target paths.</p><div class="choice-list">${tools.map(t => `<label class="choice"><input type="checkbox" data-toggle="${t.id}" ${state.checked.has(t.id) ? "checked" : ""} ${t.status === "drift" ? "disabled" : ""}/><div><div class="choice-title">${t.name}</div><div class="choice-copy">${t.note}</div></div>${status(t)}</label>`).join("")}</div>`;
  if (state.step === 1) return `<h2>Review decisions</h2><p class="lede">Only tools with an existing user choice need a decision.</p><div class="decision"><strong>Claude Code currently uses “auto”</strong><p class="choice-copy">Kaku would manage only the theme selector and two official custom-theme files.</p><div class="radio-row"><label><input type="radio" name="take" value="keep" ${state.takeover === "keep" ? "checked" : ""}/> Keep current theme and skip Claude Code</label><label><input type="radio" name="take" value="take" ${state.takeover === "take" ? "checked" : ""}/> Take over theme selection; restore “auto” when removed</label></div></div><div class="decision"><strong>Starship uses palette “catppuccin”</strong><p class="choice-copy">It stays unchecked unless you explicitly choose takeover in the tool inspector.</p></div>`;
  if (state.step === 2) return `<h2>Preview exact changes</h2><p class="lede">No files have been changed. Apply rechecks every path and value before writing.</p><div class="preview-list">${tools.filter(t => state.checked.has(t.id) || (t.id === "claude" && state.takeover === "take")).map(t => `<div class="preview-item"><strong>${t.name}</strong> · ${t.timing}<div class="choice-copy">${t.scope}</div>${t.targets.map(x => `<div class="preview-path">${x}</div>`).join("")}</div>`).join("")}</div>`;
  return "";
}

function renderB() {
  const step = state.results ? 4 : state.step;
  const names = ["Detect", "Decide", "Preview", "Apply"];
  return `<main class="shell guided-wrap">
    ${renderTop("Set up tool themes", "A guided path separates safe installs from choices that replace an existing theme.")}
    <div class="steps">${names.map((n, i) => `<div class="step ${i === step ? "active" : i < step ? "done" : ""}">${i + 1}. ${n}</div>`).join("")}</div>
    <section class="panel guided-card">${stepBody()}</section>
    <div class="footer-actions">${state.step > 0 && !state.results ? `<button class="button" data-action="back">Back</button>` : ""}<button class="button">Cancel</button>${state.results ? `<button class="button primary" data-action="reset">Done</button>` : state.step < 2 ? `<button class="button primary" data-action="next">Continue</button>` : `<button class="button primary" data-action="apply">Apply ${state.checked.size + (state.takeover === "take" ? 1 : 0)} adapters</button>`}</div>
  </main>`;
}

function card(t, action) {
  return `<article class="tool-card ${t.id === state.selected ? "focus" : ""}"><div class="card-top"><span class="card-name">${t.name}</span>${status(t)}</div><div class="card-copy">${t.note}<br>${t.timing}</div><div class="card-actions">${action}</div></article>`;
}

function renderC() {
  const safe = tools.filter(t => t.status === "safe");
  const decision = tools.filter(t => ["takeover", "shadowed"].includes(t.status));
  const managed = tools.filter(t => t.status === "drift");
  return `<main class="shell">
    ${renderTop("Theme coordination workbench", "Set up new tools and maintain existing adapters from one status-oriented view.")}
    <section class="workbench-grid">
      <div class="panel lane"><div class="lane-head"><h2>Ready</h2><span class="lane-count">${safe.length}</span></div>${safe.map(t => card(t, `<button class="mini" data-toggle="${t.id}">${state.checked.has(t.id) ? "Selected" : "Select"}</button><button class="mini" data-select="${t.id}">Inspect</button>`)).join("")}</div>
      <div class="panel lane"><div class="lane-head"><h2>Needs your decision</h2><span class="lane-count">${decision.length}</span></div>${decision.map(t => card(t, `<button class="mini" data-select="${t.id}">Review scope</button>${t.status === "takeover" ? `<button class="mini" data-toggle="${t.id}">Take over</button>` : ""}`)).join("")}</div>
      <div class="panel lane"><div class="lane-head"><h2>Managed & attention</h2><span class="lane-count">${managed.length + 2}</span></div>${managed.map(t => card(t, `<button class="mini" data-action="resolve">Resolve drift</button>`)).join("")}${card({name:"Yazi", id:"yazi", status:"safe", state:"Built-in", note:"Follows Kaku appearance automatically", timing:"Live"}, `<button class="mini">Details</button>`)}${card({name:"Atuin", id:"atuin", status:"safe", state:"Built-in", note:"Existing Kaku coordination", timing:"Live"}, `<button class="mini">Details</button>`)}</div>
    </section>
    <div class="panel activity"><div><strong>${state.checked.size} adapters selected</strong><div class="tiny subtle">Changes are previewed and applied one tool at a time.</div></div><div><button class="button">Status JSON</button> <button class="button primary" data-action="preview">Review changes</button></div></div>
  </main>`;
}

function switcher(v) {
  return `<div class="proto-switcher"><button data-variant="prev">←</button><div class="label">${v} — ${variants[v]}</div><button data-variant="next">→</button></div><div class="proto-note">THROWAWAY PROTOTYPE · no config writes</div>`;
}

function modal(title, copy, actions) {
  const existing = document.querySelector(".modal-wrap"); if (existing) existing.remove();
  const el = document.createElement("div"); el.className = "modal-wrap";
  el.innerHTML = `<div style="position:fixed;inset:0;background:rgba(0,0,0,.72);display:grid;place-items:center;z-index:90"><div class="panel" style="width:min(620px,90vw);padding:20px"><h2>${title}</h2><div class="lede">${copy}</div><div class="footer-actions">${actions}<button class="button" data-action="close-modal">Close</button></div></div></div>`;
  document.body.appendChild(el); bind();
}

function bind() {
  document.querySelectorAll("[data-select]").forEach(el => el.onclick = e => { if (e.target.matches("input,button")) return; state.selected = el.dataset.select; render(); });
  document.querySelectorAll("input[data-toggle]").forEach(el => el.onchange = e => { e.stopPropagation(); toggle(el.dataset.toggle); });
  document.querySelectorAll("button[data-toggle]").forEach(el => el.onclick = e => { e.stopPropagation(); toggle(el.dataset.toggle); });
  document.querySelectorAll("input[name=take]").forEach(el => el.onchange = () => { state.takeover = el.value; });
  document.querySelectorAll("[data-action]").forEach(el => el.onclick = () => {
    const a = el.dataset.action;
    if (a === "next") { state.step++; render(); }
    if (a === "back") { state.step--; render(); }
    if (a === "apply") { state.results = true; render(); }
    if (a === "reset") { state.step = 0; state.results = false; render(); }
    if (a === "preview" || a === "preview-takeover") modal("Preview authorization scope", "Kaku will recheck every selector, asset hash, and real path before writing. Existing values are restored when the adapter is removed; outside changes suspend synchronization.", `<button class="button primary">Looks clear</button>`);
    if (a === "resolve") modal("btop changed outside Kaku", "Expected color_theme = Kaku, but found gruvbox. Choose Re-take over to preserve the original baseline, Accept current and release, or Adopt current as the new baseline.", `<button class="button">Re-take over</button><button class="button">Release</button><button class="button">New baseline</button>`);
    if (a === "close-modal") document.querySelector(".modal-wrap")?.remove();
  });
  document.querySelectorAll("[data-variant]").forEach(el => el.onclick = () => cycle(el.dataset.variant === "next" ? 1 : -1));
}

function cycle(delta) {
  const keys = Object.keys(variants); const i = keys.indexOf(variant()); setVariant(keys[(i + delta + keys.length) % keys.length]);
}

function render() {
  const v = variant(); document.getElementById("app").innerHTML = (v === "A" ? renderA() : v === "B" ? renderB() : renderC()) + switcher(v); bind();
}

addEventListener("keydown", e => { if (["INPUT", "TEXTAREA"].includes(document.activeElement?.tagName) || document.activeElement?.isContentEditable) return; if (e.key === "ArrowLeft") cycle(-1); if (e.key === "ArrowRight") cycle(1); });
addEventListener("popstate", render);
render();
