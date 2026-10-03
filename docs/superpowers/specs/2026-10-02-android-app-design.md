# tmls Android app — design

Date: 2026-10-02. Status: approved direction (user: "I want an Android app … tested on the Samsung,
updated with the update button, like my other apps").

## Why

The browser version works on a phone but Brave's chrome, tabs and gestures get in the way. The app
is the same page, full screen, with a persistent login and the in-app update the user's other
apps have (hub `archbox` app, fin).

## Shape

```
┌─ tmls (app) ───────────────────────────────┐   first run
│ Update available · Install   ← banner      │   ┌─ Connect to tmls ─────────────┐
│ ☰ Terminal Sketch  archbox·Season-36 🔔 ⎋ ⟳App│ │ Home address  [http://…:8794] │
│ $ claude                                   │   │ Away address  [https://…    ] │
│ > fix the parser                           │   │ (optional, tried when home    │
│                                            │   │  doesn't answer)              │
│ Esc Tab Ctrl ↑ ↓ ← → / - |                 │   │            [Connect]          │
│ [ keyboard ]                               │   └───────────────────────────────┘
└────────────────────────────────────────────┘
```

- **WebView of tmls-web** (the phone layout that already exists), no pull-to-refresh, no zoom,
  dark background, `adjustResize` so the keyboard shrinks the page. The site's own login page is
  used; the WebView's cookie jar keeps the session (30 days).
- **Servers:** two addresses typed once (Setup screen): home and, optionally, away. On every start
  the app tries `/healthz` on home (3 s) then away, and loads the first that answers; neither →
  "Can't reach tmls" with Retry / Change server. No address is built in: the repo is public.
- **Update** (ported from fin's `Updater.kt`/`UpdateRules.kt`, itself from the hub app): on start and
  every 6 h the app fetches `/app/latest.json` with the site cookie; a strictly higher `versionCode`
  shows the banner; a tap downloads `/app/tmls.apk`, checks length + SHA-256, hands it to
  `PackageInstaller` (Android asks to confirm). The site shows a **⟳ App** header button to the
  app's user agent (`TmlsApp/1`); it navigates to `/app/update`, which the app intercepts as a
  manual check ("Already up to date" / install).
- **Server side** (tmls-web): `GET /app/latest.json`, `GET /app/tmls.apk`, `GET /app/update` (303 →
  `/`), all behind the login, files from `--apk-dir` (default `~/.config/tmls/apk`). In the
  container: `~/stacks/tmls/apk` mounted at `/home/tmls/apk`.

## Build, test, install (archbox)

Same layout as `hub/android`: `android/Dockerfile.build` (JDK 17 + SDK 35 + Gradle 8.9, nothing on the
host), `in-docker.sh`, `build.sh` (unit tests, lint, assemble, zipalign + apksigner), `make.sh`
(version code = max(last+1, minutes since 2024-01-01); keystore and `last-version-code` in
`~/.config/tmls-android/`; publishes `tmls.apk` + `latest.json` atomically to `~/stacks/tmls/apk/`),
`test.sh`, `install.sh` (the Samsung over USB adb on the host: install, launch, screenshot to
`android/out/launch.png`), `adb.sh shot NAME`.

Unit tests (plain JUnit, no Android): `UpdateRules` (manifest parsing, newer, digest, copy),
`Servers.normalize`, `Reach.first` (MockWebServer). Device test: install on the Samsung, screenshot,
log in, open a session, swipe, keyboard, update banner against a bumped `latest.json`.

## Not in scope

A native terminal (xterm.js in the WebView is the terminal); the TV; iOS.
