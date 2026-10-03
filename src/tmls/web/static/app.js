"use strict";
// tmls web: rows on the left (like the terminal UI), the selected session's terminal beside them.
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
let newWait = null;            // waits for a just-created session's row; any select() ends it
let alerts = [];               // newest last, kept after a look
let unreadCount = 0;
let fontSize = Math.min(FONT_MAX, Math.max(FONT_MIN, Number(store.get("tmls-font")) || 14));

// ---- terminal ----
const term = new Terminal({ fontSize, cursorBlink: true, scrollback: 5000,
  fontFamily: "'JetBrains Mono', 'DejaVu Sans Mono', monospace", theme: { background: "#15191f" } });
const fit = new FitAddon.FitAddon();
term.loadAddon(fit);
term.open($("term"));
// WebGL draws the terminal on the GPU: a full redraw (tmux resizing, scrolling) costs a fraction
// of the DOM renderer's. Without WebGL, or if the GPU drops the context, the DOM renderer stays.
try {
  const webgl = new WebglAddon.WebglAddon();
  webgl.onContextLoss(() => webgl.dispose());
  term.loadAddon(webgl);
} catch (e) { /* the DOM renderer it is */ }
let sock = null, retries = 0, retryTimer = null;

function sendSize() {
  if (sock && sock.readyState === WebSocket.OPEN) sock.send(JSON.stringify({ t: "size", cols: term.cols, rows: term.rows }));
}
function refit() { if (!$("term").hidden) { fit.fit(); sendSize(); } }
// A phone keyboard sliding in resizes the pane every frame: fit at most once a frame, and tell the
// server (which resizes tmux, redrawing the whole window) only once the size has settled.
let fitFrame = 0, sizeTimer = null;
new ResizeObserver(() => {
  if (fitFrame || $("term").hidden) return;
  fitFrame = requestAnimationFrame(() => {
    fitFrame = 0;
    fit.fit();
    clearTimeout(sizeTimer);
    sizeTimer = setTimeout(sendSize, 150);
  });
}).observe($("pane"));
function sendKeys(d) { if (sock && sock.readyState === WebSocket.OPEN) sock.send(JSON.stringify({ t: "in", d })); }
let ctrlArmed = false;  // the key bar's Ctrl: the next typed key becomes a control character
term.onData((d) => {
  if (ctrlArmed && d.length === 1) { d = String.fromCharCode(d.toUpperCase().charCodeAt(0) & 0x1f); armCtrl(false); }
  sendKeys(d);
});

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
  // Not over the phone's open list: focusing the terminal there pops the keyboard over the rows.
  ws.onopen = () => { overlay(null); refit(); if (!$("rows").classList.contains("open")) term.focus(); };
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
    if (retries >= 5) { overlay("connection lost", true); return; }
    retries += 1;
    overlay("reconnecting…");
    retryTimer = setTimeout(() => attach(key), 2000);
  };
}

function select(key) {
  if (!rows.has(key)) return;
  clearInterval(newWait);  // a manual pick wins over the row we were waiting for
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
  openRows(false);
}

// ---- phones: rows as a drawer, a key bar ----
const phone = matchMedia("(max-width: 700px)");
function openRows(open) {
  $("rows").classList.toggle("open", open);
  $("shade").classList.toggle("open", open);
}
$("menu").onclick = () => openRows(!$("rows").classList.contains("open"));
$("shade").onclick = () => openRows(false);
// The Android app resizes the page once per keyboard move, not every frame; while the keyboard
// slides it reports how far it is above the page's bottom, and the key bar rides on top of it.
window.tmlsKeyboard = (px) => { $("keys").style.transform = px > 0 ? `translateY(${-px}px)` : ""; };
// The Android app's Back button: close what is open, and on a phone go from a session back to the
// list. False means nothing was left to close, and the app leaves.
window.tmlsBack = () => {
  if (!$("new").hidden) { closeNew(); return true; }
  if (!$("alerts").hidden) { $("alerts").hidden = true; return true; }
  if (phone.matches && current && !$("rows").classList.contains("open")) { openRows(true); return true; }
  return false;
};
const KEYS = { Escape: "\x1b", Tab: "\t", Up: "\x1b[A", Down: "\x1b[B", Left: "\x1b[D", Right: "\x1b[C" };
function armCtrl(on) {
  ctrlArmed = on;
  document.querySelector('#keys button[data-key="Ctrl"]').classList.toggle("on", on);
}
// A swipe over the terminal scrolls the shell: one wheel tick per line of movement. The
// program gets wheel reports when it asked for the mouse (tmux `mouse on` scrolls its
// history), the alternate screen gets arrow keys (what a wheel does), and the normal
// screen is xterm's own scrollback, which xterm scrolls by itself.
let touchY = null, touchX = 0;
function wheelStep(up, x, y) {
  if (term.modes.mouseTrackingMode !== "none") {
    const r = $("term").querySelector(".xterm-screen").getBoundingClientRect();
    const col = Math.max(0, Math.min(term.cols - 1, Math.floor((x - r.left) / (r.width / term.cols))));
    const row = Math.max(0, Math.min(term.rows - 1, Math.floor((y - r.top) / (r.height / term.rows))));
    sendKeys(`\x1b[<${up ? 64 : 65};${col + 1};${row + 1}M`);
  } else if (term.buffer.active.type === "alternate") {
    sendKeys(up ? "\x1b[A" : "\x1b[B");
  }
}
$("term").addEventListener("touchstart", (e) => { touchY = e.touches[0].clientY; touchX = e.touches[0].clientX; }, { passive: true });
$("term").addEventListener("touchend", () => { touchY = null; });
$("term").addEventListener("touchmove", (e) => {
  if (touchY === null) return;
  e.preventDefault();
  const screen = $("term").querySelector(".xterm-screen");
  const line = (screen ? screen.getBoundingClientRect().height / term.rows : 0) || 16;
  const y = e.touches[0].clientY;
  while (Math.abs(y - touchY) >= line) {
    const up = y > touchY;  // finger down: older content, like a wheel up
    wheelStep(up, touchX, y);
    touchY += up ? line : -line;
  }
}, { passive: false });

for (const b of document.querySelectorAll("#keys button")) {
  b.onpointerdown = (e) => e.preventDefault();  // the terminal keeps the focus (and the keyboard)
  b.onclick = () => {
    const key = b.dataset.key;
    if (key === "Ctrl") armCtrl(!ctrlArmed);
    else sendKeys(KEYS[key] || key);
    term.focus();
  };
}
$("reattach").onclick = () => { retries = 0; if (current) { term.reset(); attach(current); } };

// ---- font zoom ----
function setFont(size) {
  fontSize = Math.min(FONT_MAX, Math.max(FONT_MIN, size));
  term.options.fontSize = fontSize;
  $("zoom-size").textContent = `${fontSize}px`;
  store.set("tmls-font", String(fontSize));
  refit();
  if (current && $("sketch").hidden) term.focus();
}
$("zoom-in").onclick = () => setFont(fontSize + 2);
$("zoom-out").onclick = () => setFont(fontSize - 2);
$("zoom-in").onmousedown = $("zoom-out").onmousedown = (e) => e.preventDefault();  // never take focus from the terminal
window.addEventListener("keydown", (e) => {
  if (!e.ctrlKey || e.altKey || e.metaKey) return;
  if (e.key === "=" || e.key === "+") { e.preventDefault(); setFont(fontSize + 2); }
  else if (e.key === "-") { e.preventDefault(); setFont(fontSize - 2); }
}, true);

// ---- rows ----
function drawTitle() {
  const row = rows.get(current);
  $("title").textContent = row ? `${row.label || row.host} · ${row.name}  ${MARK[row.mark]} ${row.mark}` : "pick a session";
}

function rowSig(row) { return JSON.stringify([row.mark, row.line, row.shown, row.online, row.pane, row.key === current]); }

function drawRows() {
  const box = $("rows");
  const old = new Map([...box.querySelectorAll(".row")].map((el) => [el.dataset.key, el]));
  const children = [];
  const byHost = new Map();
  for (const row of rows.values()) {
    if (!byHost.has(row.host)) byHost.set(row.host, []);
    byHost.get(row.host).push(row);
  }
  for (const [host, list] of byHost) {
    const head = document.createElement("div");
    head.className = "host" + (list.every((r) => !r.online) ? " offline" : "");
    head.textContent = list[0].label || host;  // "local" is the machine tmls runs on
    if (list.some((r) => r.online)) {
      const add = document.createElement("button");
      add.className = "add";
      add.title = "New session";
      add.textContent = "+";
      add.onclick = (e) => { e.stopPropagation(); openNew(host, list[0].label || host); };
      head.append(add);
    }
    children.push(head);
    for (const row of list.sort((a, b) => a.name.localeCompare(b.name))) {
      const sig = rowSig(row), prev = old.get(row.key);
      if (prev && prev.dataset.sig === sig) { children.push(prev); continue; }  // same node: a click in flight still lands
      const el = rowEl(row);
      el.dataset.sig = sig;
      children.push(el);
    }
  }
  box.replaceChildren(...children);
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
  el.append(top);
  const waiting = row.mark === "waiting" && row.shown;
  if (waiting && row.shown.length) {
    const prompt = document.createElement("div");
    prompt.className = "prompt";
    for (const text of row.shown.slice(0, 8)) {
      const div = document.createElement("div");
      div.textContent = text;
      prompt.append(div);
    }
    el.title = row.shown.join("\n");
    el.append(prompt);
  } else {
    const line = document.createElement("div");
    line.className = "line";
    line.textContent = row.line || "";
    line.title = row.line || "";
    el.append(line);
  }
  if (waiting) {
    const actions = document.createElement("div");
    actions.className = "actions";
    for (const [label, yes] of [["Yes", true], ["No", false]]) {
      const b = document.createElement("button");
      b.className = yes ? "yes" : "no";
      b.textContent = label;
      // stopPropagation skips the outside-click handler, so close the alerts list here
      b.onclick = (e) => { e.stopPropagation(); $("alerts").hidden = true; answer(row, yes); };
      actions.append(b);
    }
    el.append(actions);
  }
  el.onclick = () => select(row.key);
  return el;
}

async function answer(row, yes) {
  const r = await fetch("/api/approve", { method: "POST", headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ host: row.host, name: row.name, shown: row.shown, yes, pane: row.pane }) });
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

// ---- new session ----
let presets = [];              // agent preset names, from /api/config
let newHost = null, autoName = "", nameTimer = null;
let newOpened = 0;             // counts openNew calls, so a late /api/create answer can't reach a newer form
// like create.default_name: the folder's last part, "home" for ~
const defaultName = (folder) => folder.replace(/^[~/]+|[~/]+$/g, "") ? folder.replace(/\/+$/, "").split("/").pop() : "home";
const postJson = (url, body) => fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });

function openNew(host, label) {
  newHost = host;
  newOpened += 1;
  clearTimeout(nameTimer);
  $("alerts").hidden = true;  // the +'s stopPropagation keeps the outside-click handler from closing it
  $("new-create").disabled = false;
  $("new-title").textContent = `New session on ${label}`;
  $("new-folder").value = "~";
  $("new-name").value = autoName = "home";
  $("new-error").textContent = "";
  const box = $("new-start");
  box.replaceChildren();
  ["shell", "claude", ...presets].forEach((start, i) => {
    const option = document.createElement("label");
    const radio = document.createElement("input");
    radio.type = "radio";
    radio.name = "new-start";
    radio.value = start;
    radio.checked = start === "claude";
    const text = document.createElement("span");
    text.textContent = start;
    option.append(radio, text);
    box.append(option);
  });
  $("new").hidden = false;
  $("new-folder").focus();
}

function closeNew() {
  $("new").hidden = true;
  clearTimeout(nameTimer);
  if (current && $("sketch").hidden) term.focus();
}

$("new-folder").oninput = () => {
  const folder = $("new-folder").value.trim();
  if ($("new-name").value === autoName) $("new-name").value = autoName = defaultName(folder);  // not edited yet: follow
  clearTimeout(nameTimer);
  nameTimer = setTimeout(() => suggestName(newHost, folder || "~"), 250);
};

async function suggestName(host, folder) {  // the host's git repo name, when Folder is inside one
  const r = await postJson("/api/suggest-name", { host, folder }).catch(() => null);
  if (!r || !r.ok) return;
  const { name } = await r.json();
  if ($("new").hidden || host !== newHost || ($("new-folder").value.trim() || "~") !== folder) return;
  if ($("new-name").value === autoName) $("new-name").value = autoName = name;
}

$("new-form").onsubmit = async (e) => {
  e.preventDefault();
  const host = newHost, name = $("new-name").value.trim(), opened = newOpened;
  const start = $("new-start").querySelector("input:checked")?.value || "shell";
  $("new-error").textContent = "";
  $("new-create").disabled = true;
  let error = null;
  try {
    const r = await postJson("/api/create", { host, folder: $("new-folder").value, name, start });
    if (r.status === 401) { location.href = "/login"; return; }
    if (!r.ok) error = (await r.json().catch(() => ({}))).error || `create failed (${r.status})`;
  } catch {
    error = "couldn't reach tmls";
  }
  if (opened !== newOpened || $("new").hidden) return;  // closed or reopened meanwhile: not this form's answer
  $("new-create").disabled = false;
  if (error) { $("new-error").textContent = error; return; }
  closeNew();
  const key = `${host}/${name}`;
  let tries = 0;
  clearInterval(newWait);
  newWait = setInterval(() => {  // the row arrives with the next poll
    if (rows.has(key)) select(key);
    else if (++tries >= 50) clearInterval(newWait);
  }, 300);
};
$("new-cancel").onclick = closeNew;
window.addEventListener("keydown", (e) => { if (e.key === "Escape" && !$("new").hidden) closeNew(); });

// ---- alerts ----
function drawBell() { $("bell-count").textContent = unreadCount ? String(unreadCount) : ""; }
$("bell").onclick = () => {
  const box = $("alerts");
  if (!box.hidden) { box.hidden = true; return; }
  box.replaceChildren();
  if (!alerts.length) {
    const none = document.createElement("div");
    none.className = "none";
    none.textContent = "No alerts yet";
    box.append(none);
  }
  for (const a of alerts.slice().reverse()) {
    const el = document.createElement("div");
    el.className = "alert";
    el.textContent = `${MARK[a.mark]} ${a.name} · ${rows.get(a.key)?.label || a.key.split("/")[0]}`;
    el.onclick = () => { box.hidden = true; select(a.key); };
    box.append(el);
  }
  unreadCount = 0;
  drawBell();
  box.hidden = false;
};
window.addEventListener("keydown", (e) => { if (e.key === "Escape") $("alerts").hidden = true; });
document.addEventListener("click", (e) => {
  if (!$("alerts").hidden && !e.target.closest("#alerts, #bell")) $("alerts").hidden = true;
});

// ---- live rows ----
let events = null;
let listShown = false;         // the phone's drawer opens by itself only on the first listing, not on every reconnect
function connectEvents() {
  const ws = new WebSocket(wsUrl("/api/events"));
  events = ws;
  ws.onopen = () => { $("feed").hidden = true; $("rows").classList.remove("stale"); };
  ws.onmessage = (e) => {
    const msg = JSON.parse(e.data);
    if (msg.t === "rows") {
      if (msg.full) rows.clear();  // a (re)connect sends everything: drop sessions that ended meanwhile
      for (const key of msg.gone) rows.delete(key);
      for (const row of msg.set) rows.set(row.key, row);
      if (!current) {
        const saved = store.get("tmls-current");
        if (saved && rows.has(saved)) select(saved);
        else if (msg.full && phone.matches && !listShown) { listShown = true; openRows(true); }  // a phone shows the list first, once
      }
      drawRows();
      drawTitle();
    } else if (msg.t === "alerts") {
      const items = msg.items.filter((a) => a.key !== current);
      alerts.push(...items);
      alerts = alerts.slice(-50);
      unreadCount = Math.min(unreadCount + items.length, alerts.length);
      drawBell();
    }
  };
  ws.onclose = (e) => {
    if (e.code === 4401) { location.href = "/login"; return; }
    if (events !== ws) return;
    $("feed").hidden = false;
    $("rows").classList.add("stale");
    setTimeout(connectEvents, 2000);
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
  presets = c.presets || [];
  if (!sketchUrls.length) { $("tab-sketch").disabled = true; $("tab-sketch").title = "Add sketchpad's URL to ~/.config/tmls/sketchpad"; }
}).catch(() => {});

function sketchUrl() {
  const row = rows.get(current);
  // Same scheme as this page: an https page can't frame an http one.
  const base = sketchUrls.find((u) => u.startsWith(location.protocol)) || null;
  if (!base) return null;
  // embed=1: sketchpad shows just the board; these rows pick the session. Sketchpad knows this
  // machine by its name, not "local".
  const sep = base.includes("?") ? "&" : "?";
  return row ? `${base}${sep}embed=1&target=${encodeURIComponent(`${row.label || row.host}/${row.name}`)}` : `${base}${sep}embed=1`;
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

// The Android app (user agent "TmlsApp/<n>") gets a button that asks it to check for an update;
// it intercepts /app/update itself. Browsers never see the button.
if (navigator.userAgent.includes("TmlsApp/")) {
  const b = document.createElement("button");
  b.id = "app-update";
  b.textContent = "⟳";
  b.title = "Check for an app update";
  b.onclick = () => { location.href = "/app/update"; };
  $("logout").closest("form").before(b);
}

setFont(fontSize);
connectEvents();
