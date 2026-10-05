"""Browser end-to-end check of tmls web: real server (fake hosts) + headless Chromium.

Run from the repo root: python3 tests/web/e2e_web.py  (needs Playwright in that python; the
server itself runs under `uv run`).
"""
import json
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path

from playwright.sync_api import sync_playwright

PORT = 8798
BASE = f"http://127.0.0.1:{PORT}"
ROOT = Path(__file__).resolve().parents[2]


def check(name, cond):
    print(("PASS " if cond else "FAIL ") + name)
    if not cond:
        raise SystemExit(1)


def term_text(page):
    return page.evaluate("(() => { const b = term.buffer.active; let s = ''; for (let i = 0; i < b.length; i++) s += b.getLine(i).translateToString(true) + ' '; return s; })()")


def get_app_checks(browser, desktop):
    """An Android browser (not the app) gets a "Get the Android app" link in the list."""
    check("a desktop browser gets no app link", desktop.locator("#get-app").count() == 0)
    ctx = browser.new_context(viewport={"width": 390, "height": 844}, is_mobile=True, has_touch=True,
                              user_agent="Mozilla/5.0 (Linux; Android 14; Pixel 8) Chrome/130 Mobile Safari/537.36")
    ctx.add_cookies(desktop.context.cookies())
    g = ctx.new_page()
    g.goto(BASE + "/")
    g.wait_for_selector("#get-app", timeout=5000)
    check("an Android browser gets the app link in the list", g.get_attribute("#get-app", "href") == "/app/tmls.apk"
          and g.is_visible("#get-app"))
    ctx.close()


def app_checks(browser, desktop):
    """The Android app's WebView (user agent "... TmlsApp/1 TmlsVersion/<name>"): the session list
    ends with its version and "Check for updates" (like fin), which goes to /app/update for the app
    to intercept; a plain browser following it just lands on the page."""
    check("a browser gets no update button", desktop.locator("#app-update").count() == 0)
    ctx = browser.new_context(viewport={"width": 390, "height": 844}, is_mobile=True, has_touch=True,
                              user_agent="Mozilla/5.0 (Linux; Android 14) Chrome/130 Mobile TmlsApp/1 TmlsVersion/1.261003.42")
    ctx.add_cookies(desktop.context.cookies())
    a = ctx.new_page()
    a.add_init_script("window.tmlsApp = { postMessage(m) { (window.__ime = window.__ime || []).push(m); } }")
    a.goto(BASE + "/")
    a.wait_for_selector("#rows #app-update")
    a.evaluate("term.focus()")
    a.wait_for_function("(window.__ime || []).at(-1) === 'ime:none'", timeout=5000)
    check("no session yet: no keyboard over the list", True)
    a.evaluate("select('box/alpha'); term.focus()")
    a.wait_for_function("(window.__ime || []).at(-1) === 'ime:terminal'", timeout=5000)
    check("the terminal focused: the app is told to type plain characters (no doubled words)", True)
    a.evaluate("window.__sent = []; const real = sock.send.bind(sock); sock.send = (d) => { window.__sent.push(d); real(d); }; tmlsType('hi')")
    touch = """(dy) => { const el = document.querySelector('.xterm-screen'), r = el.getBoundingClientRect();
      const t = (y) => new Touch({ identifier: 1, target: el, clientX: r.left + 40, clientY: y });
      el.dispatchEvent(new TouchEvent('touchstart', { touches: [t(r.top + 100)], bubbles: true, cancelable: true }));
      if (dy) el.dispatchEvent(new TouchEvent('touchmove', { touches: [t(r.top + 100 + dy)], bubbles: true, cancelable: true }));
      el.dispatchEvent(new TouchEvent('touchend', { touches: [], bubbles: true, cancelable: true })); }"""
    a.evaluate("window.__ime = []")
    a.evaluate(touch, 120)
    check("scrolling the terminal doesn't bring the keyboard up", "ime:terminal" not in a.evaluate("window.__ime"))
    a.evaluate(touch, 0)
    check("a tap on the terminal does", a.evaluate("window.__ime.at(-1)") == "ime:terminal")
    a.click('#keys button[data-key="Keyboard"]')
    check("the key bar's keyboard button asks the app to show or hide it", a.evaluate("window.__ime.at(-1)") == "ime:toggle")
    a.evaluate("showSketch()")
    check("Sketch hands the keyboard back to the page (its fields type normally)", a.evaluate("window.__ime.at(-1)") == "ime:text")
    a.evaluate("showTerminal()")
    link = "https://example.com/a/very/long/path/that/wraps/over/two/lines/on/a/phone?x=1"
    a.evaluate(f"term.reset(); term.write('see {link} ok\\r\\n')")
    a.wait_for_timeout(200)
    a.evaluate("window.__ime = []")
    tap_at = """([col, row]) => { const el = document.querySelector('.xterm-screen'), r = el.getBoundingClientRect();
      const x = r.left + (col + 0.5) * r.width / term.cols, y = r.top + (row + 0.5) * r.height / term.rows;
      const t = new Touch({ identifier: 2, target: el, clientX: x, clientY: y });
      el.dispatchEvent(new TouchEvent('touchstart', { touches: [t], changedTouches: [t], bubbles: true, cancelable: true }));
      el.dispatchEvent(new TouchEvent('touchend', { touches: [], changedTouches: [t], bubbles: true, cancelable: true })); }"""
    cols = a.evaluate("term.cols")
    a.evaluate(tap_at, [2, 1])  # the link's second row: it wrapped
    check(f"tapping a link (even its wrapped part) opens it through the app ({a.evaluate('window.__ime')})",
          a.evaluate("window.__ime") == ["open\n" + link] and cols < len(link) + 4)
    a.evaluate("window.__ime = []")
    a.evaluate(tap_at, [cols - 1, 3])  # empty space: the usual keyboard
    check("tapping beside it is just a tap", a.evaluate("window.__ime") == ["ime:terminal"])
    check("the app's own input reaches the terminal (tmlsType)",
          a.evaluate("window.__sent.map(d => JSON.parse(d)).filter(f => f.t === 'in').map(f => f.d)") == ["hi"])
    a.evaluate("window.__sent = []")
    a.click('#keys button[data-key="Paste"]')
    check("Paste asks the app for the phone's clipboard", a.evaluate("window.__ime.at(-1)") == "paste")
    a.evaluate("tmlsPaste('pasted text')")
    check("and what the app hands back goes to the terminal as one paste",
          a.evaluate("window.__sent.map(d => JSON.parse(d)).filter(f => f.t === 'in').map(f => f.d).join('')") == "pasted text")
    a.evaluate("term.write('line one\\r\\ncopy-me-please\\r\\n')")
    a.click('#keys button[data-key="Copy"]')
    check("Copy puts the keyboard away", a.evaluate("window.__ime.includes('ime:none')"))
    check("Copy shows the terminal's text, selectable", a.is_visible("#copyview")
          and "copy-me-please" in a.inner_text("#copy-text")
          and "\n\n\n" not in a.inner_text("#copy-text")
          and a.evaluate("getComputedStyle(document.getElementById('copy-text')).userSelect") != "none")
    a.click("#copy-all")
    check("Copy all hands the text to the app's clipboard",
          a.evaluate("window.__ime.at(-1).startsWith('copy\\n') && window.__ime.at(-1).includes('copy-me-please')"))
    check("Back closes the copy view first", a.evaluate("tmlsBack()") is True and a.is_hidden("#copyview"))
    a.evaluate("document.querySelector('.host .add').click()")
    a.wait_for_selector("#new:not([hidden])")
    a.evaluate("window.__ime = []")
    a.dispatch_event("#new-folder", "pointerdown")
    check("touching a text field tells the app at once (its focus event waits for the app)",
          a.evaluate("window.__ime.at(-1)") == "ime:text")
    a.focus("#new-folder")
    a.wait_for_function("(window.__ime || []).at(-1) === 'ime:text'", timeout=5000)
    check("a normal field focused: suggestions come back", True)
    a.keyboard.press("Escape")
    a.evaluate("openRows(true)")
    check("the app's list ends with Check for updates", a.inner_text("#app-update").strip() == "Check for updates"
          and a.is_visible("#app-update"))
    check("next to the installed version", "1.261003.42" in a.inner_text("#app-version"))
    check("the header has no ⟳ any more", a.locator("header #app-update").count() == 0)
    check("the app itself gets no app link", a.locator("#get-app").count() == 0)
    with a.expect_request(lambda r: r.url.endswith("/app/update")):
        a.click("#app-update")
    a.wait_for_load_state()
    check(f"Check for updates goes to /app/update, which lands back on the page ({a.url})", a.url == BASE + "/")
    ctx.close()


def mobile_checks(browser, desktop):
    """A phone: the rows are a drawer behind ☰, the terminal gets the width, and a key bar
    supplies Esc, Tab, Ctrl and arrows. Same login (cookies), fresh storage (no saved session)."""
    ctx = browser.new_context(viewport={"width": 390, "height": 844}, device_scale_factor=3,
                              is_mobile=True, has_touch=True)
    ctx.add_cookies(desktop.context.cookies())
    m = ctx.new_page()
    m.goto(BASE + "/")
    m.wait_for_selector('.row[data-key="box/alpha"]')
    check("phone: with nothing selected the rows drawer is open", m.evaluate("document.getElementById('rows').classList.contains('open')"))
    m.click('.row[data-key="box/alpha"]')
    m.wait_for_function("!document.getElementById('rows').classList.contains('open')")
    check("phone: picking a session closes the drawer", True)
    check("phone: the terminal gets the full width", m.evaluate("document.getElementById('pane').offsetWidth") >= 380)
    m.wait_for_function("(() => { const b = term.buffer.active; let s = ''; for (let i = 0; i < b.length; i++) s += b.getLine(i).translateToString(true) + ' '; return s; })().includes('attached-alpha')")
    m.click("#menu")
    m.wait_for_selector("#rows.open")
    check("phone: ☰ opens the drawer", True)
    m.click("#shade", position={"x": 370, "y": 400})  # the strip beside the drawer (it opens from the left)
    m.wait_for_function("!document.getElementById('rows').classList.contains('open')")
    check("phone: tapping beside the drawer closes it", True)
    m.evaluate("window.__sent = []; const real = sock.send.bind(sock); sock.send = (d) => { window.__sent.push(d); real(d); }; 0")
    for key in ("Escape", "Tab", "Up"):
        m.click(f'#keys button[data-key="{key}"]')
    sent = m.evaluate("window.__sent.filter(d => typeof d === 'string').map(d => JSON.parse(d)).filter(f => f.t === 'in').map(f => f.d)")
    check("phone: the key bar sends Esc, Tab and arrows", sent == ["\x1b", "\t", "\x1b[A"])
    check("phone: the key bar leaves the terminal focused", m.evaluate("document.activeElement.className.includes('xterm-helper-textarea')"))
    m.click('#keys button[data-key="Ctrl"]')
    m.keyboard.type("c")
    sent = m.evaluate("window.__sent.filter(d => typeof d === 'string').map(d => JSON.parse(d)).filter(f => f.t === 'in').map(f => f.d)")
    check("phone: Ctrl then c sends ^C, and Ctrl disarms", sent[-1] == "\x03" and not m.evaluate("document.querySelector('#keys button[data-key=\"Ctrl\"]').classList.contains('on')"))
    # a swipe over the terminal scrolls the shell, never the page: arrows on the alternate screen,
    # wheel reports when the program asked for the mouse
    swipe = """(dy) => { const el = document.querySelector('.xterm-screen'), r = el.getBoundingClientRect();
      const t = (y) => new Touch({ identifier: 1, target: el, clientX: r.left + 40, clientY: y });
      el.dispatchEvent(new TouchEvent('touchstart', { touches: [t(r.top + 100)], bubbles: true, cancelable: true }));
      el.dispatchEvent(new TouchEvent('touchmove', { touches: [t(r.top + 100 + dy)], bubbles: true, cancelable: true }));
      el.dispatchEvent(new TouchEvent('touchend', { touches: [], bubbles: true, cancelable: true })); }"""
    # ^C above ended the fake session's cat: attach again, and wrap the new socket
    m.evaluate("select('box/alpha')")
    m.wait_for_function("sock && sock.readyState === 1 && (() => { const b = term.buffer.active; let s = ''; for (let i = 0; i < b.length; i++) s += b.getLine(i).translateToString(true) + ' '; return s; })().includes('attached-alpha')")
    m.evaluate("window.__sent = []; const real2 = sock.send.bind(sock); sock.send = (d) => { window.__sent.push(d); real2(d); }; 0")
    m.evaluate("window.__sent = []; term.write('\\x1b[?1049h')")
    m.wait_for_function("term.buffer.active.type === 'alternate'")
    line = m.evaluate("document.querySelector('.xterm-screen').getBoundingClientRect().height / term.rows")
    m.evaluate(swipe, 3 * line + 2)  # finger down: older content, like a wheel up
    sent = m.evaluate("window.__sent.filter(d => typeof d === 'string').map(d => JSON.parse(d)).filter(f => f.t === 'in').map(f => f.d)")
    check("phone: swiping down over the alternate screen sends arrow-up keys", sent == ["\x1b[A"] * 3)
    check("phone: the page itself can't be pulled to refresh", m.evaluate("getComputedStyle(document.body).overscrollBehaviorY") == "none")
    m.evaluate("window.__sent = []; term.write('\\x1b[?1000h\\x1b[?1006h')")
    m.wait_for_function("term.modes.mouseTrackingMode !== 'none'")
    m.evaluate(swipe, -(2 * line + 2))  # finger up: newer content, wheel down
    sent = m.evaluate("window.__sent.filter(d => typeof d === 'string').map(d => JSON.parse(d)).filter(f => f.t === 'in').map(f => f.d)")
    check("phone: with the mouse on, a swipe sends wheel reports to the program", len(sent) == 2 and all(d.startswith("\x1b[<65;") and d.endswith("M") for d in sent))
    m.evaluate("term.write('\\x1b[?1000l\\x1b[?1049l')")
    m.evaluate("window.__sent = []")
    for h in range(844, 504, -34):  # a keyboard sliding in shrinks the page every frame
        m.set_viewport_size({"width": 390, "height": h})
        m.wait_for_timeout(16)
    m.wait_for_timeout(500)
    sizes = m.evaluate("window.__sent.filter(d => typeof d === 'string').map(d => JSON.parse(d)).filter(f => f.t === 'size')")
    check(f"phone: a keyboard animation sends the server one size, once it settles ({sizes})",
          len(sizes) == 1 and sizes[0]["rows"] == m.evaluate("term.rows"))
    m.evaluate("tmlsKeyboard(120)")
    check("phone: while the app's keyboard slides, the key bar rides on it (no relayout)",
          m.evaluate("getComputedStyle(document.getElementById('keys')).transform") == "matrix(1, 0, 0, 1, 0, -120)")
    m.evaluate("tmlsKeyboard(0)")
    check("phone: and settles back when it stops", m.evaluate("getComputedStyle(document.getElementById('keys')).transform") == "none")
    check("phone: Back on a session goes back to the list", m.evaluate("tmlsBack()") is True
          and m.evaluate("document.getElementById('rows').classList.contains('open')"))
    check("phone: Back on the list leaves the app", m.evaluate("tmlsBack()") is False)
    m.evaluate("""Object.defineProperty(document, 'hidden', {value: true, configurable: true});
                  document.dispatchEvent(new Event('visibilitychange'))""")
    check("phone: out of sight (app in the background, screen off) it lets go of the session",
          m.evaluate("sock === null"))
    m.evaluate("""Object.defineProperty(document, 'hidden', {value: false, configurable: true});
                  document.dispatchEvent(new Event('visibilitychange'))""")
    m.wait_for_function("sock && sock.readyState === 1", timeout=10000)
    check("phone: and attaches again when it's looked at", m.evaluate("rows.has(current)"))
    m.evaluate("tmlsVisible(false)")
    check("phone: the app going to the background lets go too (tmlsVisible)", m.evaluate("sock === null"))
    m.evaluate("tmlsVisible(true)")
    m.wait_for_function("sock && sock.readyState === 1", timeout=10000)
    check("phone: and coming back attaches again", True)
    m.reload()
    m.wait_for_selector('.row[data-key="box/alpha"]')
    m.wait_for_timeout(500)
    check("phone: opening the app shows the list and attaches nothing (other screens keep their size)",
          m.evaluate("sock === null && current === null")
          and m.evaluate("document.getElementById('rows').classList.contains('open')"))
    check("phone: the last session is marked in the list",
          m.locator(".row.last").count() == 1)
    m.evaluate("select('box/alpha')")
    m.wait_for_function("sock && sock.readyState === 1", timeout=10000)
    m.evaluate("openRows(true)")  # where the checks below expect the list
    m.evaluate("showSketch()")
    check("phone: the key bar is hidden while Sketch shows", m.is_hidden("#keys"))
    m.evaluate("showTerminal()")
    check("phone: and back with the terminal", m.is_visible("#keys"))
    m.evaluate("document.activeElement.blur(); sock.close()")  # the app back from the background: a reconnect
    m.wait_for_function("sock && sock.readyState === 1", timeout=10000)
    m.wait_for_timeout(300)
    check("phone: a reconnect with the list open doesn't pop the keyboard over it",
          not m.evaluate("document.activeElement.className.includes('xterm-helper-textarea')"))
    ctx.close()


def main():
    folder = Path(tempfile.mkdtemp(prefix="tmls-e2e-"))
    srv = subprocess.Popen(["uv", "run", "python", "tests/web/e2e_server.py", str(folder), str(PORT)], cwd=ROOT)
    try:
        for _ in range(100):
            try:
                urllib.request.urlopen(BASE + "/healthz", timeout=1)
                break
            except OSError:
                time.sleep(0.1)
        with sync_playwright() as p:
            browser = p.chromium.launch(channel="chrome", headless=True)
            page = browser.new_page(viewport={"width": 1400, "height": 800})
            errors = []
            page.on("pageerror", lambda e: errors.append(str(e)))

            page.goto(BASE + "/")
            check("/ sends you to the login page", page.url.endswith("/login"))
            page.fill('input[name="username"]', "tester")
            page.fill('input[name="password"]', "nope")
            page.press('input[name="password"]', "Enter")
            page.wait_for_selector("#error:has-text('Wrong')")
            check("wrong password shows the error on the form and keeps the username",
                  page.input_value('input[name="username"]') == "tester")
            page.fill('input[name="username"]', "tester")
            page.fill('input[name="password"]', "pw")
            page.press('input[name="password"]', "Enter")
            page.wait_for_selector('.row[data-key="box/alpha"]')
            page.wait_for_selector('.host[data-host="spare"] .add', timeout=5000)
            check("a host with no sessions still shows its header with +", True)
            check("logged in: rows for alpha and beta",
                  page.locator('.row[data-key="box/beta"]').count() == 1)
            check("plain session shows its last screen line",
                  page.inner_text('.row[data-key="box/alpha"] .line') == "build ok")
            page.evaluate("""window.__el = document.querySelector('.row[data-key="box/alpha"]')""")
            page.wait_for_timeout(1500)  # several fake poll ticks with nothing changing
            page.evaluate("drawRows()")  # no rows message arrives meanwhile: redraw so the client is tested too
            check("an unchanged row keeps its element across poll ticks",
                  page.evaluate("""document.querySelector('.row[data-key="box/alpha"]') === window.__el"""))
            page.evaluate("""events.onmessage({data: JSON.stringify({t: 'rows', gone: [],
              set: [{...rows.get('box/alpha'), line: 'changed'}]})})""")
            check("a changed row gets a new element showing the change",
                  page.evaluate("""(() => { const el = document.querySelector('.row[data-key="box/alpha"]');
                                    return el !== window.__el && el.querySelector('.line').textContent === 'changed'; })()"""))
            prompt_lines = page.evaluate("""[...document.querySelectorAll('.row[data-key="box/beta"] .prompt > div')]
                                            .map((d) => d.textContent)""")
            check(f"a waiting row shows its prompt line by line ({prompt_lines})", prompt_lines == ["Bash", "rm x"])
            check("the waiting row's tooltip is the prompt",
                  "rm x" in (page.get_attribute('.row[data-key="box/beta"]', "title") or ""))

            page.click('.row[data-key="box/alpha"]')
            page.wait_for_function("(() => { const b = term.buffer.active; let s = ''; for (let i = 0; i < b.length; i++) s += b.getLine(i).translateToString(true) + ' '; return s; })().includes('attached-alpha')")
            check("clicking a row attaches its terminal", True)
            page.click("#term")
            page.keyboard.type("hello-e2e")
            page.keyboard.press("Enter")
            page.wait_for_function("(() => { const b = term.buffer.active; let s = ''; for (let i = 0; i < b.length; i++) s += b.getLine(i).translateToString(true) + ' '; return s; })().split('hello-e2e').length > 2")
            check("typing reaches the session (and comes back)", True)

            page.click("#zoom-in")
            page.click("#zoom-in")
            check("A+ twice makes the font 18px", page.inner_text("#zoom-size") == "18px")
            page.keyboard.type("zoomtype")
            page.keyboard.press("Enter")
            page.wait_for_function("(() => { const b = term.buffer.active; let s = ''; for (let i = 0; i < b.length; i++) s += b.getLine(i).translateToString(true) + ' '; return s; })().includes('zoomtype')", timeout=5000)
            check("after A+ typing still reaches the terminal", True)
            page.reload()
            page.wait_for_selector('.row[data-key="box/alpha"]')
            check("font size survives a reload", page.inner_text("#zoom-size") == "18px")
            page.wait_for_function("(() => { const b = term.buffer.active; let s = ''; for (let i = 0; i < b.length; i++) s += b.getLine(i).translateToString(true) + ' '; return s; })().includes('attached-alpha')")
            check("reload reattaches the last session", True)

            page.click('.row[data-key="box/beta"] button.yes')
            page.wait_for_timeout(500)
            lines = (folder / "approved.jsonl").read_text().splitlines()
            check("Yes sends the prompt it showed", json.loads(lines[-1]) == ["box", "beta", ["Bash", "rm x"], True])
            (folder / "prompt.json").write_text(json.dumps(["Edit", "app.py"]))
            page.evaluate("""() => {  // the row still shows the old prompt when Yes is clicked
              const b = document.querySelector('.row[data-key="box/beta"] button.yes'); b.click(); }""")
            page.wait_for_selector("#toast:not([hidden])")
            check("a changed prompt shows a toast instead of answering", "isn't asking" in page.inner_text("#toast"))

            dlink = "https://example.com/desktop-link"
            page.evaluate(f"window.__opened = []; window.open = (u) => {{ window.__opened.push(u); }}; term.write('\\r\\n{dlink}\\r\\n')")
            page.wait_for_timeout(200)
            cell = page.evaluate("""(() => { const b = term.buffer.active;
              for (let y = 0; y < term.rows; y++) { const t = b.getLine(b.viewportY + y).translateToString(true), x = t.indexOf('https://example.com/desktop-link');
                if (x >= 0) { const r = document.querySelector('.xterm-screen').getBoundingClientRect();
                  return [r.left + (x + 3.5) * r.width / term.cols, r.top + (y + 0.5) * r.height / term.rows]; } } return null; })()""")
            page.mouse.click(cell[0], cell[1])
            check(f"clicking a link in the terminal opens it in a new tab ({page.evaluate('window.__opened')})",
                  page.evaluate("window.__opened") == [dlink])
            page.click("#tab-sketch")
            src = page.get_attribute("#sketch iframe", "src")
            check("Sketch tab frames sketchpad's board only, targeted at the session",
                  src.endswith("embed=1&target=box%2Fbeta") or src.endswith("embed=1&target=box%2Falpha"))
            page.evaluate("window.__frame = document.querySelector('#sketch iframe')")
            check(f"Sketch has its own Send-to picker, on the current session ({page.input_value('#sketch-target')})",
                  page.input_value("#sketch-target") == page.evaluate("current")
                  and page.locator("#sketch-target option").count() >= 2)
            other = "box/alpha" if page.evaluate("current") == "box/beta" else "box/beta"
            page.evaluate("""window.__posted = []; const f = document.querySelector('#sketch iframe');
              const real = f.contentWindow.postMessage.bind(f.contentWindow);
              f.contentWindow.postMessage = (m, o) => { window.__posted.push(m); };""")
            page.select_option("#sketch-target", other)
            check("picking another session keeps the same board (no new frame, so the drawing stays)",
                  page.evaluate("document.querySelector('#sketch iframe') === window.__frame")
                  and page.evaluate("current") == other and page.is_visible("#sketch"))
            check(f"and tells sketchpad the new target ({page.evaluate('window.__posted')})",
                  page.evaluate("window.__posted.at(-1)") == {"type": "tmls-target", "target": other})
            page.click("#tab-terminal")
            check("Terminal tab comes back", page.is_visible("#term") and not page.is_visible("#sketch"))
            page.click("#tab-sketch")
            check("and Sketch again is the same board", page.evaluate("document.querySelector('#sketch iframe') === window.__frame"))
            page.click("#tab-terminal")

            page.evaluate("events.close()")
            page.wait_for_selector("#rows.stale", timeout=5000)
            check("a lost live feed dims the rows and says it is reconnecting",
                  page.is_visible("#feed") and "reconnecting" in page.inner_text("#feed"))
            page.wait_for_selector("#rows:not(.stale)", timeout=10000)
            check("the feed comes back by itself", not page.is_visible("#feed"))

            page.evaluate("""events.onmessage({data: JSON.stringify({t: 'alerts', items: [
              {key: 'box/gamma', name: 'gamma', mark: 'done'}, {key: 'box/delta', name: 'delta', mark: 'failed'}]})})""")
            check("new alerts are counted on the bell", page.inner_text("#bell-count") == "2")
            page.click("#bell")
            check("the bell lists the alerts", page.locator("#alerts .alert").count() == 2)
            page.click("#bell")
            page.click("#bell")
            check("alerts are still listed after a look, and none are unread",
                  page.locator("#alerts .alert").count() == 2 and page.inner_text("#bell-count") == "")
            page.keyboard.press("Escape")
            check("Esc closes the alert list", page.is_hidden("#alerts"))
            page.click("#bell")
            page.click("#title")
            check("a click outside closes the alert list", page.is_hidden("#alerts"))

            page.click(".host .add")
            page.wait_for_selector("#new:not([hidden])")
            check("+ on box's header opens New session on box", page.inner_text("#new-title") == "New session on box")
            page.fill("#new-folder", "~/code/thing")
            check("Name follows Folder", page.input_value("#new-name") == "thing")
            page.wait_for_function("document.getElementById('new-name').value === 'repo-thing'", timeout=5000)
            check("then becomes the host's repo name", True)
            starts = page.evaluate("[...document.querySelectorAll('#new-start label')].map((l) => l.textContent.trim())")
            check(f"Start offers shell, claude and the presets ({starts})", starts == ["shell", "claude", "Opus plan"])
            checked = page.evaluate("document.querySelector('#new-start input:checked').value")
            check(f"the first preset is the default Start ({checked})", checked == "Opus plan")
            page.fill("#new-name", "alpha")
            page.click("#new-create")
            page.wait_for_selector("#new-error:has-text('already exists')")
            check("a name in use shows the server's reason and the modal stays open", page.is_visible("#new"))
            page.fill("#new-name", "gamma")
            page.press("#new-name", "Enter")
            page.wait_for_selector('.row[data-key="box/gamma"].current', timeout=10000)
            check("Enter creates gamma, closes the modal and selects the new row", page.is_hidden("#new"))
            page.wait_for_function("(() => { const b = term.buffer.active; let s = ''; for (let i = 0; i < b.length; i++) s += b.getLine(i).translateToString(true) + ' '; return s; })().includes('attached-gamma')")
            check("the terminal attaches to the new session", True)
            page.click(".host .add")
            page.fill("#new-name", "delta")
            page.press("#new-name", "Enter")
            page.wait_for_selector("#new", state="hidden")
            page.click('.row[data-key="box/alpha"]')
            page.wait_for_timeout(1500)  # delta's row arrives meanwhile
            check("picking another row right after Create keeps that row selected",
                  page.get_attribute(".row.current", "data-key") == "box/alpha")
            page.click("#bell")
            page.evaluate("document.querySelector('.host .add').click()")  # the open list covers box's +
            page.wait_for_selector("#new:not([hidden])")
            check("+ closes the alert list", page.is_hidden("#alerts"))
            page.keyboard.press("Escape")
            check("Esc closes the New session modal", page.is_hidden("#new"))

            # ⋯ on a row: rename (the open session follows its new name), then kill in two steps
            page.click('.row[data-key="box/gamma"]')
            page.wait_for_selector('.row[data-key="box/gamma"].current')
            other_ctx = browser.new_context()  # another device on the same session: it must follow too
            other_ctx.add_cookies(page.context.cookies())
            other = other_ctx.new_page()
            other.goto(BASE + "/")
            other.wait_for_selector('.row[data-key="box/gamma"]')
            other.click('.row[data-key="box/gamma"]')
            other.wait_for_function("(() => { const b = term.buffer.active; let s = ''; for (let i = 0; i < b.length; i++) s += b.getLine(i).translateToString(true) + ' '; return s; })().includes('attached-gamma')")
            other.evaluate("window.attaches = 0; const W = window.WebSocket; window.WebSocket = function (u, p) { if (u.includes('/api/term')) window.attaches++; return new W(u, p); }; window.WebSocket.prototype = W.prototype; window.WebSocket.OPEN = W.OPEN;")
            page.click('.row[data-key="box/gamma"] .more')
            page.wait_for_selector("#manage:not([hidden])")
            check("⋯ opens the dialog with the session's name", page.input_value("#manage-name") == "gamma")
            check("⋯ doesn't select the row", page.get_attribute(".row.current", "data-key") == "box/gamma")
            page.fill("#manage-name", "a:b")
            page.click("#manage-rename")
            page.wait_for_selector("#manage-error:has-text('can')")
            check("a bad name shows the reason and the dialog stays", page.is_visible("#manage"))
            page.fill("#manage-name", "gamma2")
            page.press("#manage-name", "Enter")
            page.wait_for_selector('.row[data-key="box/gamma2"].current', timeout=10000)
            check("rename closes the dialog and the open session follows its new name",
                  page.is_hidden("#manage") and page.locator('.row[data-key="box/gamma"]').count() == 0)
            other.wait_for_selector('.row[data-key="box/gamma2"].current', timeout=5000)
            check("another tab on it follows the new name too, without reattaching",
                  other.evaluate("window.attaches") == 0 and "gamma2" in other.inner_text("#title"))
            page.click('.row[data-key="box/gamma2"] .more')
            check("Back (the Android app's) closes the dialog, not the app", page.evaluate("window.tmlsBack()") and page.is_hidden("#manage"))
            page.click('.row[data-key="box/gamma2"] .more')
            page.click("#manage-kill")
            page.wait_for_selector("#manage-kill:has-text('Confirm kill')")
            check("the first Kill press only asks", page.locator('.row[data-key="box/gamma2"]').count() == 1
                  and "Kill gamma2?" in page.inner_text("#manage-warning"))
            page.click("#manage-kill")
            page.wait_for_selector('.row[data-key="box/gamma2"]', state="detached")
            check("Confirm kill removes the row and leaves nothing selected",
                  page.is_hidden("#manage") and page.locator(".row.current").count() == 0 and page.is_visible("#empty"))
            other.wait_for_selector("#empty", state="visible", timeout=5000)
            check("the other tab learns it ended at once", other.locator('.row[data-key="box/gamma2"]').count() == 0
                  and other.evaluate("current") is None)
            other_ctx.close()
            page.click('.row[data-key="box/beta"] .more')
            page.click("#manage-kill")
            page.wait_for_selector("#manage-warning:has-text('Kill beta?')")
            check("Kill names what is still running", "Running: claude" in page.inner_text("#manage-warning"))
            page.fill("#manage-name", "is#sue")
            page.click("#manage-rename")
            page.wait_for_selector("#manage-error:has-text('#')")
            check("a # in a name is refused (tmux would expand it)", page.is_visible("#manage"))
            page.keyboard.press("Escape")
            check("Esc closes the dialog", page.is_hidden("#manage"))

            mobile_checks(browser, page)  # before the logout: it reuses this login's cookies
            app_checks(browser, page)
            get_app_checks(browser, page)
            status = page.evaluate("fetch('/logout', {method: 'POST', redirect: 'manual'}).then(r => r.type + ' ' + r.status)")
            cookies = [c["name"] for c in page.context.cookies()]
            page.goto(BASE + "/")
            check(f"after logout the page asks for login again ({status}, cookies={cookies}, url={page.url})",
                  page.url.endswith("/login"))
            check("no page errors", not errors)
            browser.close()
    finally:
        srv.terminate()
        srv.wait()


if __name__ == "__main__":
    sys.exit(main())
