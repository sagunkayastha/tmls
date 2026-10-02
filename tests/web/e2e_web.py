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
            check("wrong password shows the error on the form", True)
            page.fill('input[name="username"]', "tester")
            page.fill('input[name="password"]', "pw")
            page.press('input[name="password"]', "Enter")
            page.wait_for_selector('.row[data-key="box/alpha"]')
            check("logged in: rows for alpha and beta",
                  page.locator('.row[data-key="box/beta"]').count() == 1)
            check("plain session shows its last screen line",
                  page.inner_text('.row[data-key="box/alpha"] .line') == "build ok")

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
            check("Sketch tab frames sketchpad targeted at the session", src.endswith("?target=box%2Fbeta") or src.endswith("?target=box%2Falpha"))
            page.click("#tab-terminal")
            check("Terminal tab comes back", page.is_visible("#term") and not page.is_visible("#sketch"))

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
