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


def app_checks(browser, desktop):
    """The Android app's WebView (user agent "... TmlsApp/1"): a ⟳ App button asks the app to
    check for an update via /app/update; a plain browser following it just lands on the page."""
    check("a browser gets no ⟳ App button", desktop.locator("#app-update").count() == 0)
    ctx = browser.new_context(viewport={"width": 390, "height": 844}, is_mobile=True, has_touch=True,
                              user_agent="Mozilla/5.0 (Linux; Android 14) Chrome/130 Mobile TmlsApp/1")
    ctx.add_cookies(desktop.context.cookies())
    a = ctx.new_page()
    a.goto(BASE + "/")
    a.wait_for_selector("#app-update")
    check("the app gets a ⟳ button", a.inner_text("#app-update").strip() == "⟳")
    a.click("#shade", position={"x": 370, "y": 400})  # the rows drawer starts open on a phone
    a.wait_for_function("!document.getElementById('rows').classList.contains('open')")
    with a.expect_request(lambda r: r.url.endswith("/app/update")):
        a.click("#app-update")
    a.wait_for_load_state()
    check(f"⟳ App goes to /app/update, which lands back on the page ({a.url})", a.url == BASE + "/")
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

            page.click("#tab-sketch")
            src = page.get_attribute("#sketch iframe", "src")
            check("Sketch tab frames sketchpad's board only, targeted at the session",
                  src.endswith("embed=1&target=box%2Fbeta") or src.endswith("embed=1&target=box%2Falpha"))
            page.click("#tab-terminal")
            check("Terminal tab comes back", page.is_visible("#term") and not page.is_visible("#sketch"))

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
            check(f"claude is the default Start ({checked})", checked == "claude")
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

            mobile_checks(browser, page)  # before the logout: it reuses this login's cookies
            app_checks(browser, page)
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
