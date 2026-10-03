# tmls Android app — handoff plan (continue on archbox)

> **For the session on archbox:** read this first, then `docs/superpowers/specs/2026-10-02-android-app-design.md`
> (the design + sketch, approved by the user), then `PROGRESS.md` (top two entries). The user said
> "continue"; the plan below was shown to and accepted by them on 2026-10-02. Work in
> `~/Others/tmls` (this checkout, branch `android`). ubu is off: nothing lives there.

## Where things are on archbox

- **This repo:** `~/Others/tmls` (branch `android`; `main` is what the container runs).
- **Reference apps to copy from (same user, same conventions):**
  - fin: `~/Others/Budget/android/` — `app/build.gradle.kts`, `build.gradle.kts`, `settings.gradle.kts`,
    `gradle.properties`, `Dockerfile`, `app/src/main/java/com/sgkayast/fin/{MainActivity.kt,ServerUrl.kt,
    Storage.kt,AndroidStorage.kt,ui/{FinApp.kt,WebScreen.kt,SetupScreen.kt,UpdateBanner.kt},update/{Updater.kt,UpdateRules.kt}}`,
    tests `app/src/test/java/com/sgkayast/fin/{update/UpdateRulesTest.kt,ServerUrlTest.kt}`, res
    `app/src/main/res/{values/themes.xml,xml/network_security_config.xml,xml/data_extraction_rules.xml,
    mipmap-anydpi-v26/ic_launcher.xml,drawable/ic_launcher_foreground.xml}`, server routes in
    `~/Others/Budget/web/app.py` (`/app/latest.json`, `/app/fin.apk`, `/app/update`), scripts
    `~/Others/Budget/scripts/{build-apk.sh,android-gradle.sh,android-keygen.sh}`.
  - hub (`archbox` app): `~/homeserver/hub/android/{make.sh,in-docker.sh,build.sh,install.sh,adb.sh,Dockerfile.build}`,
    keys in `~/.config/hub-android/`, published to `~/stacks/hub/apk/{archbox.apk,latest.json}`.
- **Docker images already there:** `hub-android-build`, `fin-android-build` (JDK 17 + SDK). Gradle
  cache volume pattern: `hub-android-gradle`. Scratch pattern: `/data/scratch/<app>-android/`.
- **The phone:** Samsung A52s, `adb -s <phone-serial>` on archbox (USB). The other adb device,
  `<tv-ip>:5555`, is an AT&T TV box: always pass `-s`. Screen 1080x2400, density 450, Android 14,
  Brave is its default browser, Wi-Fi <phone-ip>. Screen timeout 30 s (raise with
  `settings put system screen_off_timeout 600000` during tests, restore 30000). It is logged in to
  tmls-web in Brave; the app will have its own cookie jar (log in once in the app).
- **tmls-web** runs as container `tmls-web` from `~/stacks/tmls/docker-compose.yml` (image built from
  this checkout: `cd ~/stacks/tmls && docker compose up -d --build`). LAN `http://<lan-ip>:8794`,
  away `https://tmls.<your-domain>` (media stack's Caddy on the personal tailnet). Credentials =
  sketchpad's (`~/.config/sketchpad/auth.json`); the password is the user's, never type it.

## State of the drafts (on branch `android`, uncommitted work was committed as "wip")

Written but NOT yet verified:
- `src/tmls/web/server.py`: `APK_DIR`, `app_manifest`, `app_apk`, `app_update`, routes `/app/latest.json`,
  `/app/tmls.apk`, `/app/update`, `--apk-dir`, `app["apk_dir"]`.
- `tests/web/test_app_update.py`: 5 tests. **First thing: `env -u NO_COLOR uv run pytest tests/web -q`** — the
  last run said "1 error during collection": probably the import `from tests.web.test_approve import client_logged_in`
  (check whether `tests/__init__.py` exists; else copy the helper into the new file).
- `src/tmls/web/static/app.js`: at the bottom, a "⟳ App" button for user agents containing `TmlsApp/`
  that navigates to `/app/update`. Needs an e2e check (Playwright context with `user_agent=... TmlsApp/1`).
- `android/Dockerfile.build` (fin's recipe + Gradle 8.9 + SDK 35), `android/in-docker.sh`, `android/build.sh`
  (modes test/debug/release; `-PtmlsVersionCode/-PtmlsVersionName`, `-PbuildDir=$TMLS_BUILD_DIR`),
  `android/make.sh` (keystore `~/.config/tmls-android/tmls.keystore`, alias `tmls`, versionCode scheme as hub,
  publishes to `~/stacks/tmls/apk/`). All four untested.

## Steps (verify each before the next; test-first for code)

1. **Server + web** (`main`-worthy, independent of the app): fix the test collection, make the 5 tests pass,
   add the e2e check for the header button, run `python3 tests/web/e2e_web.py` (48 checks today) — note
   on archbox Playwright may not be installed for `python3`; `pip install --user playwright && playwright install chrome`
   or skip e2e there and keep the unit tests. Commit.
2. **Android project** under `android/` (package `dev.sagun.tmls`, app name "tmls", minSdk 29, compileSdk 35,
   AGP 8.7.3, Kotlin 2.0.21, Compose BOM 2024.12.01, okhttp 4.12.0, kotlinx-serialization 1.7.3 — copy fin's
   Gradle files and change names; versionCode/Name from `-Ptmls…` project properties with `1`/`dev` defaults):
   - `Servers.kt`: `normalize(input)` (fin's `ServerUrl.normalize`), `PrefsServers` (home, away in SharedPreferences).
   - `Reach.kt`: `suspend fun first(urls: List<String>, client: OkHttpClient): String?` — GET `<url>/healthz`
     with a 3 s timeout, in order; first 200 wins. Test with MockWebServer (one dead port, then a mock).
   - `ui/SetupScreen.kt`: Home address, Away address (optional), Connect → saves and retries.
   - `ui/WebScreen.kt`: fin's minus `SwipeRefreshLayout` (no pull-to-refresh!), `userAgentString += " TmlsApp/1"`,
     `/app/update` → `onUpdateRequested`, other-origin links → browser, `setBackgroundColor(0xFF15191F)`,
     `overScrollMode = OVER_SCROLL_NEVER`, `settings.setSupportZoom(false)`, `WebView.setWebContentsDebuggingEnabled(BuildConfig.DEBUG)`.
     No cookie minting: the site's login page is shown in the WebView; `CookieManager` persists it.
   - `ui/TmlsApp.kt`: Loading → (no servers) Setup → Reach → Web (with `UpdateBanner` above) | Unreachable (Retry / Change server).
   - `update/UpdateRules.kt`, `update/Updater.kt`: fin's, with `fin`→`tmls`, APK `tmls.apk`, action
     `dev.sagun.tmls.INSTALL_RESULT`, prefs `updater`; `baseUrl` = the server that answered.
   - `MainActivity.kt`: `enableEdgeToEdge()`, `updater.checkIfDue()` in `onStart`, `close()` in `onDestroy`.
   - Manifest: INTERNET, REQUEST_INSTALL_PACKAGES, `windowSoftInputMode="adjustResize"`, `configChanges`
     as fin, `networkSecurityConfig` allowing cleartext (the LAN address is typed by the user, so base-config
     `cleartextTrafficPermitted="true"` with a comment), `allowBackup="false"`, data_extraction_rules as fin.
   - Theme `Theme.Tmls` (DeviceDefault.DayNight, no action bar); adaptive icon with a `>_` foreground.
   - Tests: `UpdateRulesTest` (fin's, renamed), `ServersTest` (normalize), `ReachTest`.
3. **Scripts**: `test.sh` (`in-docker.sh bash /w/build.sh test`), `install.sh` (host adb over USB:
   `adb -s ${DEVICE:-<phone-serial>} install -r out/<apk>`, `am start -n dev.sagun.tmls/.MainActivity`, screencap
   → `android/out/launch.png`; `APK=` picks a file; debug package is `dev.sagun.tmls.debug`), `adb.sh shot NAME`.
   `android/.gitignore`: `out/`, `build/`, `.gradle/`, `local.properties`. Make the four scripts executable.
4. **Toolchain proof:** `android/make.sh debug` (first run downloads the SDK + Gradle deps: minutes). Fix until
   `out/tmls-debug.apk` exists. Then `android/test.sh` green.
5. **Device:** `android/install.sh APK=out/tmls-debug.apk` → `out/launch.png` shows Setup. Fill the two
   addresses with `adb shell input text` (home `http://<lan-ip>:8794`, away `https://tmls.<your-domain>`),
   Connect → the site's login page; **the user logs in on the phone** (ask them). Then: rows, open a throwaway
   session (create one with `+`, e.g. `tmls-app`; never attach to New_Vision/Season-36: attaching resizes
   their windows), keyboard, key bar, swipe-scroll (archbox tmux has `mouse on`), Back button, rotate.
   Screenshots to `android/out/shots/`. Kill the throwaway afterwards.
6. **Release + update flow:** `android/make.sh` (creates `~/.config/tmls-android/` keystore; publishes to
   `~/stacks/tmls/apk/`). Add to `~/stacks/tmls/docker-compose.yml`: volume `./apk:/home/tmls/apk:ro` and
   `--apk-dir /home/tmls/apk` in `command`; `docker compose up -d`. Install the release APK on the phone
   (uninstall the debug one first: different package, can coexist, but test the release). Then `make.sh`
   again → a higher versionCode is published → in the app tap "⟳ App" → banner → Install → Android's
   confirm → the new version runs. Also check the 6 h auto-check path by reading `Updater`.
7. **Docs + merge:** README "Android app" section (build, install, update, keys backup), spec/plan updated,
   PROGRESS.md entry, merge `android` into `main`, push, `docker compose up -d --build`. Then hand over:
   the user installs from the phone via `https://tmls.<your-domain>/app/tmls.apk` (logged in) or the
   first install over adb, and tests.

## Gotchas learned today

- `docker compose exec -T tmls-web ssh …` hangs until ControlPersist expires; use `-o ControlMaster=no -o ControlPath=none`.
- `adb shell am start -a android.intent.action.VIEW -d URL` opens a NEW Brave tab each time.
- `adb shell input text` goes through the phone's shell: `$(…)` expands there, spaces need `%s`.
- The web's `/static` is behind the login: `curl` without the cookie gets a 302, not the file.
- Hosts with zero tmux sessions have no `+` in the web (known gap).
- Repo is public: no real hostnames/IPs/tokens in committed files (deploy files use example addresses).
