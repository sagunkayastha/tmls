"use strict";
// tmls web: rows on the right, the selected session's terminal on the left.
const $ = (id) => document.getElementById(id);
const MARK = { running: "●", waiting: "?", done: "◆", failed: "✕", idle: "○" };
const FONT_MIN = 10, FONT_MAX = 28;
const store = {
  get(k) { try { return localStorage.getItem(k); } catch { return null; } },
  set(k, v) { try { localStorage.setItem(k, v); } catch { /* private window */ } },
};
const wsUrl = (path) => `${location.protocol === "https:" ? "wss" : "ws"}://${location.host}${path}`;

const rows = new Map();        // key -> row from the server
let current = null;            // selected key
let unread = [];               // alerts not yet looked at
let fontSize = Math.min(FONT_MAX, Math.max(FONT_MIN, Number(store.get("tmls-font")) || 14));

// ---- terminal ----
const term = new Terminal({ fontSize, cursorBlink: true, scrollback: 5000,
  fontFamily: "'JetBrains Mono', 'DejaVu Sans Mono', monospace", theme: { background: "#15191f" } });
const fit = new FitAddon.FitAddon();
term.loadAddon(fit);
term.open($("term"));
let sock = null, retries = 0, retryTimer = null;

function sendSize() {
  if (sock && sock.readyState === WebSocket.OPEN) sock.send(JSON.stringify({ t: "size", cols: term.cols, rows: term.rows }));
}
function refit() { if (!$("term").hidden) { fit.fit(); sendSize(); } }
new ResizeObserver(refit).observe($("pane"));
term.onData((d) => { if (sock && sock.readyState === WebSocket.OPEN) sock.send(JSON.stringify({ t: "in", d })); });

function overlay(text, reattach = false) {
  $("overlay").hidden = !text;
  $("overlay-text").textContent = text || "";
  $("reattach").hidden = !reattach;
}

function attach(key) {
  clearTimeout(retryTimer);
  if (sock) { sock.onclose = null; sock.close(); }
  const row = rows.get(key);
  if (!row) return;
  const ws = new WebSocket(wsUrl(`/api/term?host=${encodeURIComponent(row.host)}&name=${encodeURIComponent(row.name)}`));
  ws.binaryType = "arraybuffer";
  sock = ws;
  ws.onopen = () => { overlay(null); refit(); term.focus(); };
  ws.onmessage = (e) => {
    if (typeof e.data === "string") {
      const msg = JSON.parse(e.data);
      if (msg.t === "exit") { ws.onclose = null; overlay("session ended", true); }
      return;
    }
    retries = 0;  // only real output counts: the server accepts, then closes, sockets it refuses
    term.write(new Uint8Array(e.data));
  };
  ws.onclose = (e) => {
    if (e.code === 4401) { location.href = "/login"; return; }
    if (e.code === 4403 || e.code === 4404) {  // refused for good: retrying can't help
      overlay(e.code === 4403 ? "refused: this page's address doesn't match the server (proxy Host header?)" : "no such session", true);
      return;
    }
    if (sock !== ws) return;
    if (retries >= 5) { overlay("session ended", true); return; }
    retries += 1;
    overlay("reconnecting…");
    retryTimer = setTimeout(() => attach(key), 2000);
  };
}

function select(key) {
  if (!rows.has(key)) return;
  current = key;
  store.set("tmls-current", key);
  $("empty").hidden = true;
  term.reset();
  retries = 0;
  attach(key);
  fetch("/api/seen", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ key }) });
  drawTitle();
  drawRows();
  if (!$("sketch").hidden) showSketch();
}
$("reattach").onclick = () => { retries = 0; if (current) attach(current); };

// ---- font zoom ----
function setFont(size) {
  fontSize = Math.min(FONT_MAX, Math.max(FONT_MIN, size));
  term.options.fontSize = fontSize;
  $("zoom-size").textContent = `${fontSize}px`;
  store.set("tmls-font", String(fontSize));
  refit();
}
$("zoom-in").onclick = () => setFont(fontSize + 2);
$("zoom-out").onclick = () => setFont(fontSize - 2);
window.addEventListener("keydown", (e) => {
  if (!e.ctrlKey || e.altKey || e.metaKey) return;
  if (e.key === "=" || e.key === "+") { e.preventDefault(); setFont(fontSize + 2); }
  else if (e.key === "-") { e.preventDefault(); setFont(fontSize - 2); }
}, true);

// ---- rows ----
function drawTitle() {
  const row = rows.get(current);
  $("title").textContent = row ? `${row.host} · ${row.name}  ${MARK[row.mark]} ${row.mark}` : "pick a session";
}

function drawRows() {
  const box = $("rows");
  box.replaceChildren();
  const byHost = new Map();
  for (const row of rows.values()) {
    if (!byHost.has(row.host)) byHost.set(row.host, []);
    byHost.get(row.host).push(row);
  }
  for (const [host, list] of byHost) {
    const head = document.createElement("div");
    head.className = "host" + (list.every((r) => !r.online) ? " offline" : "");
    head.textContent = host;
    box.append(head);
    for (const row of list.sort((a, b) => a.name.localeCompare(b.name))) box.append(rowEl(row));
  }
}

function rowEl(row) {
  const el = document.createElement("div");
  el.className = "row" + (row.key === current ? " current" : "") + (row.online ? "" : " offline");
  el.dataset.key = row.key;
  const top = document.createElement("div");
  const mark = document.createElement("span");
  mark.className = `mark ${row.mark}`;
  mark.textContent = MARK[row.mark] || "";
  const name = document.createElement("span");
  name.className = "name";
  name.textContent = row.name;
  top.append(mark, name);
  const line = document.createElement("div");
  line.className = "line";
  line.textContent = row.line || "";
  line.title = row.line || "";
  el.append(top, line);
  if (row.mark === "waiting" && row.shown) {
    line.textContent = row.shown.join(" · ") || row.line;
    const actions = document.createElement("div");
    actions.className = "actions";
    for (const [label, yes] of [["Yes", true], ["No", false]]) {
      const b = document.createElement("button");
      b.className = yes ? "yes" : "no";
      b.textContent = label;
      b.onclick = (e) => { e.stopPropagation(); answer(row, yes); };
      actions.append(b);
    }
    el.append(actions);
  }
  el.onclick = () => select(row.key);
  return el;
}

async function answer(row, yes) {
  const r = await fetch("/api/approve", { method: "POST", headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ host: row.host, name: row.name, shown: row.shown, yes }) });
  if (r.status === 401) { location.href = "/login"; return; }
  if (!r.ok) toast((await r.json().catch(() => ({}))).error || `approve failed (${r.status})`);
}

let toastTimer = null;
function toast(text) {
  $("toast").textContent = text;
  $("toast").hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { $("toast").hidden = true; }, 4000);
}

// ---- alerts ----
function drawBell() { $("bell-count").textContent = unread.length ? String(unread.length) : ""; }
$("bell").onclick = () => {
  const box = $("alerts");
  if (!box.hidden) { box.hidden = true; return; }
  box.replaceChildren();
  if (!unread.length) {
    const none = document.createElement("div");
    none.className = "none";
    none.textContent = "No new alerts";
    box.append(none);
  }
  for (const a of unread.slice().reverse()) {
    const el = document.createElement("div");
    el.className = "alert";
    el.textContent = `${MARK[a.mark]} ${a.name} · ${a.key.split("/")[0]}`;
    el.onclick = () => { box.hidden = true; select(a.key); };
    box.append(el);
  }
  unread = [];
  drawBell();
  box.hidden = false;
};

// ---- live rows ----
let events = null;
function connectEvents() {
  const ws = new WebSocket(wsUrl("/api/events"));
  events = ws;
  ws.onmessage = (e) => {
    const msg = JSON.parse(e.data);
    if (msg.t === "rows") {
      if (msg.full) rows.clear();  // a (re)connect sends everything: drop sessions that ended meanwhile
      for (const key of msg.gone) rows.delete(key);
      for (const row of msg.set) rows.set(row.key, row);
      if (!current) {
        const saved = store.get("tmls-current");
        if (saved && rows.has(saved)) select(saved);
      }
      drawRows();
      drawTitle();
    } else if (msg.t === "alerts") {
      unread.push(...msg.items.filter((a) => a.key !== current));
      drawBell();
    }
  };
  ws.onclose = (e) => {
    if (e.code === 4401) { location.href = "/login"; return; }
    if (events === ws) setTimeout(connectEvents, 2000);
  };
}

// Leaving the page must hang up its tmux client, even when Chrome keeps the page in its
// back-forward cache with sockets open; coming back from that cache reconnects.
window.addEventListener("pagehide", () => {
  clearTimeout(retryTimer);
  if (sock) { sock.onclose = null; sock.close(); sock = null; }
  if (events) { const ws = events; events = null; ws.close(); }
});
window.addEventListener("pageshow", (e) => {
  if (!e.persisted) return;
  connectEvents();
  if (current) { retries = 0; attach(current); }
});

// ---- sketch (Task 7 fills this in) ----
function showTerminal() {
  $("sketch").hidden = true;
  $("term").hidden = false;
  $("empty").hidden = Boolean(current);
  $("tab-terminal").classList.add("on");
  $("tab-sketch").classList.remove("on");
  refit();
}
let sketchUrls = [];
fetch("/api/config").then((r) => r.json()).then((c) => {
  sketchUrls = c.sketchpad || [];
  if (!sketchUrls.length) { $("tab-sketch").disabled = true; $("tab-sketch").title = "Add sketchpad's URL to ~/.config/tmls/sketchpad"; }
}).catch(() => {});

function sketchUrl() {
  const row = rows.get(current);
  // Same scheme as this page: an https page can't frame an http one.
  const base = sketchUrls.find((u) => u.startsWith(location.protocol)) || null;
  if (!base) return null;
  return row ? `${base}${base.includes("?") ? "&" : "?"}target=${encodeURIComponent(`${row.host}/${row.name}`)}` : base;
}

function showSketch() {
  const url = sketchUrl();
  if (!url) {  // no address with this page's scheme: open sketchpad by itself instead
    if (sketchUrls[0]) window.open(sketchUrls[0], "_blank", "noopener");
    return;
  }
  const box = $("sketch");
  if (box.dataset.url !== url) {
    const frame = document.createElement("iframe");
    frame.src = url;
    frame.allow = "clipboard-read; clipboard-write; display-capture";
    const link = document.createElement("a");  // if sketchpad's login doesn't reach the frame
    link.href = url;
    link.target = "_blank";
    link.rel = "noopener";
    link.className = "sketch-open";
    link.textContent = "Open sketchpad in a new tab ↗";
    box.replaceChildren(link, frame);
    box.dataset.url = url;
  }
  $("term").hidden = true;
  box.hidden = false;
  $("empty").hidden = true;
  $("tab-sketch").classList.add("on");
  $("tab-terminal").classList.remove("on");
}
$("tab-terminal").onclick = showTerminal;
$("tab-sketch").onclick = showSketch;

setFont(fontSize);
connectEvents();
