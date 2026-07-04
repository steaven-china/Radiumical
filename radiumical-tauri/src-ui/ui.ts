import { store, type DisplayItem, type Panel } from "./state";
import { renderMarkdown } from "./markdown";
import { filterCommands, type Command, commands } from "./commands";
import { api } from "./api";

const ASCII_LOGO = `██████╗  █████╗ ██████╗ ██╗██╗   ██╗███╗   ███╗██╗ ██████╗ █████╗ ██╗
██╔══██╗██╔══██╗██╔══██╗██║██║   ██║████╗ ████║██║██╔════╝██╔══██╗██║
██████╔╝███████║██║  ██║██║██║   ██║██╔████╔██║██║██║     ███████║██║
██╔══██╗██╔══██║██║  ██║██║██║   ██║██║╚██╔╝██║██║██║     ██╔══██║██║
     ██║  ██║██║  ██║██████╔╝██║╚██████╔╝██║ ╚═╝ ██║██║╚██████╗██║  ██║███████╗
     ╚═╝  ╚═╝╚═╝  ╚═╝╚═════╝ ╚═╝ ╚═════╝ ╚═╝     ╚═╝╚═╝ ╚═════╝╚═╝  ╚═╝╚══════╝`;

export function mountApp(root: HTMLElement) {
  root.innerHTML = `
    <div id="layout">
      <div id="sidebar">
        <div id="sidebar-header">
          <pre class="logo-ascii">${ASCII_LOGO}</pre>
          <button id="sidebar-close">&times;</button>
        </div>
        <div id="sidebar-nav">
          <button class="nav-btn active" data-panel="sessions">Sessions</button>
          <button class="nav-btn" data-panel="settings">Settings</button>
        </div>
        <div id="sidebar-content"></div>
      </div>
      <div id="main">
        <div id="toolbar">
          <div class="toolbar-left">
            <button id="menu-btn" class="icon-btn">&#9776;</button>
            <span id="status-indicator" class="status-dot"></span>
            <span id="status-text">ready</span>
          </div>
          <div class="toolbar-center"><span id="model-label">&mdash;</span></div>
          <div class="toolbar-right">
            <select id="mode-select">
              <option value="auto">Auto</option>
              <option value="plan">Plan</option>
              <option value="exec">Exec</option>
            </select>
            <button id="new-btn" class="icon-btn">+</button>
          </div>
        </div>
        <div id="welcome">
          <pre class="welcome-logo">${ASCII_LOGO}</pre>
          <div class="welcome-sub">Rust-native agentic coding assistant</div>
          <div class="welcome-hints">
            <div class="hint-chip" data-hint="Fix the bug in src/main.rs">Fix a bug</div>
            <div class="hint-chip" data-hint="Write tests for the auth module">Write tests</div>
            <div class="hint-chip" data-hint="Refactor the database layer to use connection pooling">Refactor code</div>
            <div class="hint-chip" data-hint="Explain how the orchestrator works">Explain code</div>
          </div>
          <div class="welcome-footer"><kbd>Enter</kbd> send &middot; <kbd>/</kbd> commands</div>
        </div>
        <div id="messages"></div>
        <div id="input-area">
          <div id="cmd-palette" class="hidden"></div>
          <textarea id="input" placeholder="Ask anything... (type / for commands)" rows="1"></textarea>
          <button id="send-btn">Send</button>
          <button id="cancel-btn" class="hidden">Stop</button>
        </div>
      </div>
    </div>
    <div id="toasts"></div>
    <div id="modal-overlay" class="modal-hidden">
      <div id="modal-box">
        <div id="modal-header"><span id="modal-title"></span><button id="modal-close">&times;</button></div>
        <div id="modal-body"></div>
      </div>
    </div>
  `;
  bindEvents();
  store.subscribe(render);
  store.init();
}

// ── Palette state ──
let paletteOpen = false;
let paletteIndex = 0;
let paletteItems: Command[] = [];

function bindEvents() {
  const input = document.getElementById("input") as HTMLTextAreaElement;
  const sendBtn = document.getElementById("send-btn")!;
  const cancelBtn = document.getElementById("cancel-btn")!;
  const menuBtn = document.getElementById("menu-btn")!;
  const newBtn = document.getElementById("new-btn")!;
  const modeSelect = document.getElementById("mode-select") as HTMLSelectElement;
  const closeBtn = document.getElementById("sidebar-close")!;

  input.addEventListener("keydown", (e) => {
    if (paletteOpen) {
      if (e.key === "ArrowDown") { e.preventDefault(); paletteIndex = Math.min(paletteIndex + 1, paletteItems.length - 1); renderPalette(); return; }
      if (e.key === "ArrowUp") { e.preventDefault(); paletteIndex = Math.max(paletteIndex - 1, 0); renderPalette(); return; }
      if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); selectPaletteItem(paletteItems[paletteIndex]); return; }
      if (e.key === "Tab") { e.preventDefault(); selectPaletteItem(paletteItems[paletteIndex]); return; }
      if (e.key === "Escape") { e.preventDefault(); closePalette(); return; }
    }
    if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); sendMessage(); }
    if (e.key === "Escape" && store.get().isRunning) { store.cancelTask(); }
  });

  input.addEventListener("input", () => {
    input.style.height = "auto";
    input.style.height = Math.min(input.scrollHeight, 200) + "px";
    updatePalette();
  });

  sendBtn.addEventListener("click", sendMessage);
  cancelBtn.addEventListener("click", () => store.cancelTask());
  menuBtn.addEventListener("click", () => store.toggleSidebar());
  newBtn.addEventListener("click", () => store.newSession());
  closeBtn.addEventListener("click", () => store.setPanel("chat"));
  modeSelect.addEventListener("change", () => store.setMode(modeSelect.value));

  document.querySelectorAll(".nav-btn").forEach((btn) => {
    btn.addEventListener("click", () => store.setPanel((btn as HTMLElement).dataset.panel as Panel));
  });
  document.querySelectorAll(".hint-chip").forEach((chip) => {
    chip.addEventListener("click", () => store.sendMessage((chip as HTMLElement).dataset.hint!));
  });
}

function sendMessage() {
  const input = document.getElementById("input") as HTMLTextAreaElement;
  const text = input.value.trim();
  if (!text) return;
  input.value = "";
  input.style.height = "auto";
  closePalette();

  if (text === "/help" || text === "/?") { store.showHelp(); return; }
  if (text === "/new") { store.newSession(); return; }
  if (text === "/clear" || text === "/cls") { store.newSession(); return; }
  if (text === "/sessions") { store.setPanel("sessions"); return; }
  if (text === "/settings" || text === "/config") { store.setPanel("settings"); return; }
  if (text === "/provider") { store.setPanel("settings"); return; }
  if (text === "/auto") { store.setMode("auto"); return; }
  if (text === "/plan") { store.setMode("plan"); return; }
  if (text === "/exec") { store.setMode("exec"); return; }
  if (text.startsWith("/model ")) { const m = text.slice(7).trim(); if (m) store.setModel(m); return; }
  if (text.startsWith("/session save")) { const n = text.slice(13).trim() || prompt("Name:") || ""; if (n) store.saveSession(n); return; }
  if (text.startsWith("/session load ")) { const n = text.slice(14).trim(); if (n) store.loadSession(n); return; }
  if (text === "/session list") { store.setPanel("sessions"); return; }
  if (text === "/retry" || text === "/r") { store.retry(); return; }

  store.sendMessage(text);
}

// ── Palette ──

function updatePalette() {
  const text = (document.getElementById("input") as HTMLTextAreaElement).value;
  if (text.startsWith("/") && !text.includes("\n")) {
    paletteItems = filterCommands(text);
    paletteIndex = 0;
    paletteItems.length > 0 ? openPalette() : closePalette();
  } else {
    closePalette();
  }
}

function openPalette() { paletteOpen = true; renderPalette(); }
function closePalette() { paletteOpen = false; document.getElementById("cmd-palette")!.classList.add("hidden"); }

function renderPalette() {
  const el = document.getElementById("cmd-palette")!;
  if (!paletteOpen || !paletteItems.length) { el.classList.add("hidden"); return; }
  el.classList.remove("hidden");
  let html = "", lastCat = "";
  for (let i = 0; i < paletteItems.length; i++) {
    const c = paletteItems[i];
    if (c.category !== lastCat) { html += `<div class="cmd-category">${c.category}</div>`; lastCat = c.category; }
    html += `<div class="cmd-item${i === paletteIndex ? " selected" : ""}" data-index="${i}"><span class="cmd-name">${c.name}</span><span class="cmd-desc">${c.description}</span></div>`;
  }
  el.innerHTML = html;
  el.querySelectorAll(".cmd-item").forEach((item) => {
    item.addEventListener("click", () => selectPaletteItem(paletteItems[parseInt((item as HTMLElement).dataset.index!)]));
    item.addEventListener("mouseenter", () => { paletteIndex = parseInt((item as HTMLElement).dataset.index!); renderPalette(); });
  });
  el.querySelector(".cmd-item.selected")?.scrollIntoView({ block: "nearest" });
}

function selectPaletteItem(cmd: Command | undefined) {
  if (!cmd) return;
  const input = document.getElementById("input") as HTMLTextAreaElement;
  input.value = cmd.name + " ";
  closePalette();
  input.focus();
}

// ── Modal ──

function openModal(title: string, bodyHtml: string) {
  const overlay = document.getElementById("modal-overlay")!;
  document.getElementById("modal-title")!.textContent = title;
  document.getElementById("modal-body")!.innerHTML = bodyHtml;
  overlay.classList.remove("modal-hidden");
  overlay.classList.add("modal-visible");
  document.getElementById("modal-close")!.onclick = closeModal;
  overlay.addEventListener("click", (e) => { if (e.target === overlay) closeModal(); });
}

function closeModal() {
  const overlay = document.getElementById("modal-overlay")!;
  overlay.classList.remove("modal-visible");
  overlay.classList.add("modal-hidden");
}

function showModelPicker(models: string[], current: string, onSelect: (m: string) => void) {
  let html = `<div class="modal-search-wrap"><input id="modal-search" type="text" placeholder="Filter..." autofocus /></div><div class="modal-list">`;
  for (const m of models) html += `<div class="modal-item${m === current ? " active" : ""}" data-model="${esc(m)}">${esc(m)}</div>`;
  html += `</div>`;
  openModal("Select Model", html);
  const list = document.querySelector("#modal-body .modal-list") as HTMLElement;
  const search = document.getElementById("modal-search") as HTMLInputElement;
  search.addEventListener("input", () => { const q = search.value.toLowerCase(); list.querySelectorAll(".modal-item").forEach((el) => { (el as HTMLElement).style.display = (el as HTMLElement).dataset.model!.toLowerCase().includes(q) ? "" : "none"; }); });
  search.focus();
  list.querySelectorAll(".modal-item").forEach((el) => { el.addEventListener("click", () => { onSelect((el as HTMLElement).dataset.model!); closeModal(); }); });
}

async function showProviderSwitchModal(providerName: string, apiBase: string) {
  openModal("Switch Provider", `<div class="modal-loading">Loading models...</div>`);
  let models: string[] = [];
  try { models = await api.fetchModelsForProvider(providerName, apiBase, ""); } catch {}
  if (!models.length) {
    document.getElementById("modal-body")!.innerHTML = `<div class="modal-form"><label>Model</label><input id="modal-model-input" type="text" placeholder="e.g. gpt-4o" autofocus /><button id="modal-confirm" class="small-btn">Switch</button></div>`;
    const input = document.getElementById("modal-model-input") as HTMLInputElement;
    const go = () => { const m = input.value.trim(); if (m) { store.switchProvider(providerName, apiBase, "", m); closeModal(); } };
    document.getElementById("modal-confirm")!.addEventListener("click", go);
    input.addEventListener("keydown", (e) => { if (e.key === "Enter") go(); });
    input.focus();
    return;
  }
  showModelPicker(models, store.get().appInfo?.model || "", (m) => store.switchProvider(providerName, apiBase, "", m));
}

function showModelPickerFromSettings() {
  const s = store.get();
  if (!s.appInfo) return;
  const info = s.appInfo;
  openModal("Select Model", `<div class="modal-loading">Loading...</div>`);
  const pname = resolveProviderName(info.api_base, s.providers) || info.provider;
  api.fetchModelsForProvider(pname, info.api_base, "").then((models) => {
    if (!models.length) {
      document.getElementById("modal-body")!.innerHTML = `<div class="modal-form"><label>Model</label><input id="modal-model-input" type="text" value="${esc(info.model)}" autofocus /><button id="modal-confirm" class="small-btn">Apply</button></div>`;
      const input = document.getElementById("modal-model-input") as HTMLInputElement;
      const go = () => { const m = input.value.trim(); if (m) { store.setModel(m); closeModal(); } };
      document.getElementById("modal-confirm")!.addEventListener("click", go);
      input.addEventListener("keydown", (e) => { if (e.key === "Enter") go(); });
      input.focus();
    } else {
      showModelPicker(models, info.model, (m) => store.setModel(m));
    }
  }).catch(() => {
    document.getElementById("modal-body")!.innerHTML = `<div class="modal-form"><label>Model</label><input id="modal-model-input" type="text" value="${esc(info.model)}" autofocus /><button id="modal-confirm" class="small-btn">Apply</button></div>`;
    const input = document.getElementById("modal-model-input") as HTMLInputElement;
    const go = () => { const m = input.value.trim(); if (m) { store.setModel(m); closeModal(); } };
    document.getElementById("modal-confirm")!.addEventListener("click", go);
    input.addEventListener("keydown", (e) => { if (e.key === "Enter") go(); });
    input.focus();
  });
}

// ── Choice Modal ──

function renderChoiceModal(choice: { id: string; mode: string; options: string[] } | null) {
  const overlay = document.getElementById("modal-overlay")!;
  if (!choice) {
    // Don't close if it's not a choice modal
    return;
  }

  const title = choice.mode === "input" ? "Input Required" : `Choose (${choice.mode})`;
  let bodyHtml = "";

  if (choice.mode === "input") {
    const prompt = choice.options[0] || "Enter value:";
    bodyHtml = `<div class="modal-form">
      <label>${esc(prompt)}</label>
      <input id="choice-input" type="text" autofocus />
      <div class="modal-actions"><button id="choice-cancel" class="small-btn">Cancel</button><button id="choice-submit" class="small-btn">Submit</button></div>
    </div>`;
  } else {
    bodyHtml = `<div class="choice-list">`;
    for (let i = 0; i < choice.options.length; i++) {
      bodyHtml += `<div class="choice-item" data-index="${i}"><span class="choice-num">${i + 1}</span><span class="choice-text">${esc(choice.options[i])}</span></div>`;
    }
    bodyHtml += `</div>`;
  }

  openModal(title, bodyHtml);

  if (choice.mode === "input") {
    const input = document.getElementById("choice-input") as HTMLInputElement;
    const submit = document.getElementById("choice-submit")!;
    const cancel = document.getElementById("choice-cancel")!;
    const go = () => { const v = input.value.trim(); if (v) { store.resolveChoice(v); closeModal(); } };
    submit.addEventListener("click", go);
    input.addEventListener("keydown", (e) => { if (e.key === "Enter") go(); });
    cancel.addEventListener("click", () => { store.cancelChoice(); closeModal(); });
    input.focus();
  } else {
    document.querySelectorAll(".choice-item").forEach((el) => {
      el.addEventListener("click", () => {
        const idx = (el as HTMLElement).dataset.index!;
        if (choice.mode === "multi") {
          el.classList.toggle("selected");
        } else {
          store.resolveChoice(String(parseInt(idx) + 1));
          closeModal();
        }
      });
    });
    // For multi mode, add a confirm button
    if (choice.mode === "multi") {
      const body = document.getElementById("modal-body")!;
      body.insertAdjacentHTML("beforeend", `<div class="modal-actions" style="padding:10px 16px"><button id="choice-confirm" class="small-btn">Confirm</button></div>`);
      document.getElementById("choice-confirm")!.addEventListener("click", () => {
        const selected: string[] = [];
        body.querySelectorAll(".choice-item.selected").forEach((el) => {
          selected.push(String(parseInt((el as HTMLElement).dataset.index!) + 1));
        });
        if (selected.length) { store.resolveChoice(selected.join(",")); closeModal(); }
      });
    }
  }
}

// ── Render ──

let renderedCount = 0;
let lastStreamContent = "";
let lastStreamType = "";
let lastWasStreaming = false;

function resolveProviderName(apiBase: string, providers: { provider: string; api_base: string }[]): string {
  const norm = apiBase.replace(/\/+$/, "").toLowerCase();
  for (const p of providers) if (p.api_base.replace(/\/+$/, "").toLowerCase() === norm) return p.provider;
  return "";
}

function render() {
  const s = store.get();

  // Welcome
  const hasItems = s.display.length > 0 || s.isRunning;
  document.getElementById("welcome")!.style.display = hasItems ? "none" : "flex";
  document.getElementById("messages")!.classList.toggle("visible", hasItems);

  // Status
  const indicator = document.getElementById("status-indicator")!;
  const statusText = document.getElementById("status-text")!;
  const sendBtn = document.getElementById("send-btn")!;
  const cancelBtn = document.getElementById("cancel-btn")!;
  if (s.isRunning) {
    indicator.className = "status-dot running";
    const streaming = s.display.find((d) => d.type === "assistant" && d.streaming);
    const reasoning = s.display.find((d) => d.type === "reasoning" && d.streaming);
    const tool = s.display.find((d) => d.type === "tool" && d.running);
    const thinking = s.display.find((d) => d.type === "thinking");
    statusText.textContent = streaming ? "streaming..." : reasoning ? "thinking..." : tool ? tool.name! : thinking ? "thinking..." : "working...";
    sendBtn.classList.add("hidden");
    cancelBtn.classList.remove("hidden");
  } else {
    indicator.className = "status-dot";
    statusText.textContent = "ready";
    sendBtn.classList.remove("hidden");
    cancelBtn.classList.add("hidden");
  }

  // Model label
  if (s.appInfo) {
    const pname = resolveProviderName(s.appInfo.api_base, s.providers) || s.appInfo.provider;
    document.getElementById("model-label")!.textContent = `${pname} / ${s.appInfo.model}`;
    const modeSelect = document.getElementById("mode-select") as HTMLSelectElement;
    if (document.activeElement !== modeSelect) modeSelect.value = s.appInfo.mode;
  }

  // Messages — incremental
  renderDisplayIncremental(s.display);

  // Sidebar
  renderSidebar(s);

  // Toasts
  document.getElementById("toasts")!.innerHTML = s.toasts.map((t) => `<div class="toast toast-${t.level}">${esc(t.message)}</div>`).join("");

  // Choice modal
  renderChoiceModal(s.pendingChoice);
}

function renderDisplayIncremental(items: DisplayItem[]) {
  const container = document.getElementById("messages")!;
  const last = items[items.length - 1];
  const isStreaming = (last?.type === "assistant" || last?.type === "reasoning") && last.streaming;

  // Streaming just ended — full rebuild to finalize
  if (lastWasStreaming && !isStreaming) {
    lastWasStreaming = false;
    fullRebuild(container, items);
    return;
  }
  lastWasStreaming = !!isStreaming;

  // Check if only the last streaming item changed content
  if (
    items.length === renderedCount &&
    isStreaming &&
    (last.content !== lastStreamContent || last.type !== lastStreamType)
  ) {
    const lastEl = container.lastElementChild as HTMLElement | null;
    if (lastEl) {
      let html = "";
      if (last.type === "reasoning") {
        html = `<div class="reasoning-block"><div class="reasoning-header">Reasoning</div><div class="reasoning-body">${renderMarkdown(last.content || "")}</div></div>`;
      } else {
        html = `<div class="msg-body">${renderMarkdown(last.content || "")}</div>`;
      }
      lastEl.innerHTML = html;
      lastEl.className = `msg msg-${last.type} streaming`;
      lastStreamContent = last.content || "";
      lastStreamType = last.type;
      requestAnimationFrame(() => { container.scrollTop = container.scrollHeight; });
      return;
    }
  }

  // Items added — append only new ones
  if (items.length > renderedCount && items.length - renderedCount < renderedCount) {
    // Append new items
    let html = "";
    for (let i = renderedCount; i < items.length; i++) {
      html += renderItem(items[i]);
    }
    container.insertAdjacentHTML("beforeend", html);
    renderedCount = items.length;
    if (last) {
      lastStreamContent = last.content || "";
      lastStreamType = last.type;
    }
    bindToggleEvents(container);
    requestAnimationFrame(() => { container.scrollTop = container.scrollHeight; });
    return;
  }

  // Full rebuild (items removed, or initial, or too many changes)
  fullRebuild(container, items);
}

function fullRebuild(container: HTMLElement, items: DisplayItem[]) {
  let html = "";
  for (const item of items) html += renderItem(item);
  container.innerHTML = html;
  renderedCount = items.length;
  const last = items[items.length - 1];
  lastStreamContent = last?.content || "";
  lastStreamType = last?.type || "";
  bindToggleEvents(container);
  requestAnimationFrame(() => { container.scrollTop = container.scrollHeight; });
}

function renderItem(item: DisplayItem): string {
  switch (item.type) {
    case "user":
      return `<div class="msg msg-user"><div class="msg-body">${esc(item.content || "")}</div></div>`;
    case "reasoning":
      return `<div class="msg msg-reasoning${item.streaming ? " streaming" : ""}"><div class="reasoning-block${item.streaming ? "" : " collapsed"}"><div class="reasoning-header">Reasoning</div><div class="reasoning-body">${renderMarkdown(item.content || "")}</div></div></div>`;
    case "assistant": {
      let cls = "msg msg-assistant";
      if (item.streaming) cls += " streaming";
      return `<div class="${cls}"><div class="msg-body">${renderMarkdown(item.content || "")}</div></div>`;
    }
    case "tool": {
      let argsPretty = "";
      try { argsPretty = JSON.stringify(JSON.parse(item.args || "{}"), null, 2); } catch { argsPretty = item.args || ""; }
      const hasErr = (item.result || "").toLowerCase().includes("error");
      return `<div class="tool-card${item.running ? " running" : " collapsed"}">
        <div class="tool-header">
          <span class="tool-badge">${item.running ? '<span class="spin">\u2699</span> ' : ""}${esc(item.name || "")}</span>
          <span class="tool-args-preview">${esc(argsPreview(argsPretty))}</span>
          ${hasErr && !item.running ? '<span class="tool-status err">error</span>' : ""}
          ${item.running ? "" : '<span class="tool-toggle">\u25B6</span>'}
        </div>
        ${item.running ? `<div class="tool-body"><div class="tool-args-section"><div class="tool-section-label">Arguments</div><div class="tool-args-content">${esc(argsPretty)}</div></div></div>` : ""}
        ${item.result ? `<div class="tool-body"><div class="tool-args-section"><div class="tool-section-label">Arguments</div><div class="tool-args-content">${esc(argsPretty)}</div></div><div class="tool-result-section"><div class="tool-section-label">Result</div><div class="tool-result-content">${renderMarkdown("```\n" + (item.result.length > 2000 ? item.result.slice(0, 2000) + "\n..." : item.result) + "\n```")}</div></div></div>` : ""}
      </div>`;
    }
    case "error":
      return `<div class="msg msg-error"><div class="msg-body">${esc(item.content || "")}</div></div>`;
    case "thinking":
      return `<div class="msg"><div class="thinking-indicator"><span class="dot-pulse"></span>Thinking...</div></div>`;
    default:
      return "";
  }
}

function bindToggleEvents(container: HTMLElement) {
  container.querySelectorAll(".tool-header").forEach((h) => { h.addEventListener("click", () => { h.closest(".tool-card")!.classList.toggle("collapsed"); }); });
  container.querySelectorAll(".reasoning-header").forEach((h) => { h.addEventListener("click", () => { h.closest(".reasoning-block")!.classList.toggle("collapsed"); }); });
}

function argsPreview(args: string): string {
  const oneLine = args.replace(/\n/g, " ").replace(/\s+/g, " ").trim();
  return oneLine.length > 100 ? oneLine.slice(0, 100) + "..." : oneLine;
}

// ── Sidebar ──

function renderSidebar(s: ReturnType<typeof store.get>) {
  const sidebar = document.getElementById("sidebar")!;
  const content = document.getElementById("sidebar-content")!;
  sidebar.classList.toggle("open", s.sidebarOpen);
  document.querySelectorAll(".nav-btn").forEach((btn) => {
    btn.classList.toggle("active", (btn as HTMLElement).dataset.panel === s.activePanel);
  });
  if (s.activePanel === "sessions") renderSessionsPanel(content, s);
  else if (s.activePanel === "settings") renderSettingsPanel(content, s);
}

function renderSessionsPanel(container: HTMLElement, s: ReturnType<typeof store.get>) {
  let html = `<div class="panel-section"><div class="panel-header"><h3>Sessions</h3><button id="save-session-btn" class="small-btn">Save</button></div><div class="session-list">`;
  if (!s.sessions.length) { html += `<div class="empty-state">No saved sessions</div>`; store.loadSessions(); }
  else for (const sess of s.sessions) {
    html += `<div class="session-item"><div class="session-name">${esc(sess.name)}</div><div class="session-meta">${esc(sess.model)} &middot; ${sess.message_count} msgs &middot; ${esc(sess.updated)}</div>${sess.description ? `<div class="session-desc">${esc(sess.description)}</div>` : ""}<div class="session-actions"><button class="small-btn load-btn" data-name="${esc(sess.name)}">Load</button><button class="small-btn danger del-btn" data-name="${esc(sess.name)}">Delete</button></div></div>`;
  }
  html += `</div></div>`;
  container.innerHTML = html;
  container.querySelectorAll(".load-btn").forEach((b) => { b.addEventListener("click", () => store.loadSession((b as HTMLElement).dataset.name!)); });
  container.querySelectorAll(".del-btn").forEach((b) => { b.addEventListener("click", () => { const n = (b as HTMLElement).dataset.name!; if (confirm(`Delete "${n}"?`)) store.deleteSession(n); }); });
  container.querySelector("#save-session-btn")?.addEventListener("click", () => { const n = prompt("Name:"); if (n) store.saveSession(n); });
}

function renderSettingsPanel(container: HTMLElement, s: ReturnType<typeof store.get>) {
  const info = s.appInfo;
  if (!info) { container.innerHTML = `<div class="panel-section"><div class="empty-state">Loading...</div></div>`; return; }
  let provHtml = "";
  if (!s.providers.length) { provHtml = `<div class="empty-state">Loading...</div>`; store.loadProviders(); }
  else for (const p of s.providers) {
    const active = info.api_base.replace(/\/+$/, "").toLowerCase() === p.api_base.replace(/\/+$/, "").toLowerCase();
    provHtml += `<div class="provider-item${active ? " active" : ""}"><div class="provider-info"><span class="provider-name">${esc(p.name)}</span><span class="provider-base">${esc(p.api_base)}</span></div>${active ? '<span class="provider-active-tag">active</span>' : `<button class="provider-switch-btn" data-provider="${esc(p.provider)}" data-base="${esc(p.api_base)}">Switch</button>`}</div>`;
  }
  container.innerHTML = `<div class="panel-section">
    <div class="panel-header"><h3>Settings</h3></div>
    <div class="settings-form">
      <label>Workspace</label><div class="setting-value">${esc(info.workspace)}</div>
      <label>API Key</label><div class="setting-row"><input id="setting-apikey" type="password" placeholder="sk-..." /><button id="setting-apikey-save" class="small-btn">Save</button></div><div class="setting-hint">Source: <code>${esc(info.api_key_source)}</code></div>
      <label>Model</label><div class="setting-row"><div class="setting-value" style="flex:1">${esc(info.model)}</div><button id="setting-model-pick" class="small-btn">Change</button></div>
      <label>Mode</label><select id="setting-mode"><option value="auto" ${info.mode === "auto" ? "selected" : ""}>Auto</option><option value="plan" ${info.mode === "plan" ? "selected" : ""}>Plan</option><option value="exec" ${info.mode === "exec" ? "selected" : ""}>Exec</option></select>
      <label>API Base</label><div class="setting-value">${esc(info.api_base)}</div>
      <label>Max Context</label><div class="setting-value">${info.max_context_tokens.toLocaleString()} tokens</div>
      <label>Timeouts</label><div class="setting-value">LLM ${info.llm_timeout_secs}s / Tool ${info.tool_timeout_secs}s</div>
    </div>
    <div class="settings-divider"></div>
    <div class="panel-header"><h3>Providers</h3></div>
    <div class="provider-list">${provHtml}</div>
  </div>`;

  container.querySelector("#setting-mode")?.addEventListener("change", (e) => store.setMode((e.target as HTMLSelectElement).value));
  container.querySelector("#setting-apikey-save")?.addEventListener("click", () => { const inp = container.querySelector("#setting-apikey") as HTMLInputElement; const k = inp.value.trim(); if (k) { store.saveApiKey(k); inp.value = ""; } });
  container.querySelector("#setting-model-pick")?.addEventListener("click", () => showModelPickerFromSettings());
  container.querySelectorAll(".provider-switch-btn").forEach((btn) => { btn.addEventListener("click", () => showProviderSwitchModal((btn as HTMLElement).dataset.provider!, (btn as HTMLElement).dataset.base!)); });
}

// ── Utils ──
function esc(s: string): string { const d = document.createElement("div"); d.textContent = s; return d.innerHTML; }
