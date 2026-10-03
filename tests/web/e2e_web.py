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
    return page.evaluate("document.querySelector('.xterm-rows').textContent")


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
            check("an unchanged row keeps its element across poll ticks",
                  page.evaluate("""document.querySelector('.row[data-key="box/alpha"]') === window.__el"""))
            prompt_lines = page.evaluate("""[...document.querySelectorAll('.row[data-key="box/beta"] .prompt > div')]
                                            .map((d) => d.textContent)""")
            check(f"a waiting row shows its prompt line by line ({prompt_lines})", prompt_lines == ["Bash", "rm x"])
            check("the waiting row's tooltip is the prompt",
                  "rm x" in (page.get_attribute('.row[data-key="box/beta"]', "title") or ""))

            page.click('.row[data-key="box/alpha"]')
            page.wait_for_function("document.querySelector('.xterm-rows').textContent.includes('attached-alpha')")
            check("clicking a row attaches its terminal", True)
            page.click("#term")
            page.keyboard.type("hello-e2e")
            page.keyboard.press("Enter")
            page.wait_for_function("document.querySelector('.xterm-rows').textContent.split('hello-e2e').length > 2")
            check("typing reaches the session (and comes back)", True)

            page.click("#zoom-in")
            page.click("#zoom-in")
            check("A+ twice makes the font 18px", page.inner_text("#zoom-size") == "18px")
            page.keyboard.type("zoomtype")
            page.keyboard.press("Enter")
            page.wait_for_function("document.querySelector('.xterm-rows').textContent.includes('zoomtype')", timeout=5000)
            check("after A+ typing still reaches the terminal", True)
            page.reload()
            page.wait_for_selector('.row[data-key="box/alpha"]')
            check("font size survives a reload", page.inner_text("#zoom-size") == "18px")
            page.wait_for_function("document.querySelector('.xterm-rows').textContent.includes('attached-alpha')")
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
