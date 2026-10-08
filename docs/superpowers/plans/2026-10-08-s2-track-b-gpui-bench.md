# S2 Track B — Docker GUI image, `s2-gpui`, `s2-bench` (for Claude, or a 2nd Codex)

> **Read first:** `docs/superpowers/plans/2026-10-08-s2-split-overview.md` (rules, ownership, sync points, **shared
> contract**) and the S2 plan `docs/superpowers/plans/2026-10-08-s2-interaction-slice.md` (Tasks 4–5 give detail; this file
> overrides it where they differ). Spec: `docs/PLAN.md` §3 Desktop GUI, §6 S2, §9, §10.

**Branch:** `spike/s2-gpui` from `main` after Task 0. **You own:** `spikes/s2-gpui/**`, `spikes/s2-bench/**`,
`tooling/docker/**`, `docs/licences.md`. Do not edit root `Cargo.toml`, `spikes/s2-core/**`, `spikes/s2-wgpu/**`.

Commit author `sgkayast <sgn.kayastha@gmail.com>`, no AI attribution. No `/tmp` (use `/data/scratch/s2/`). No deletes/moves
without the user's yes. PC jobs: write the exact commands, then wait for the user's yes.

---

## B1 — Docker image with GUI build deps — **priority: Track A's A2 needs it**

- [ ] 1. `tooling/docker/Dockerfile.build`: add `libxkbcommon-dev libwayland-dev libvulkan-dev libx11-dev libxcursor-dev
      libxi-dev libxrandr-dev libfontconfig1-dev libfreetype-dev cmake clang` (wgpu/winit set). gpui extras are found in B2
      and added there.
- [ ] 2. Commit `docker: GUI build deps for s2 front-ends`; cherry-pick-able alone (Track A may need it before B merges —
      Claude merges B1 to `main` directly after review).
- [ ] 3. PC image rebuild (**ask user**): `ssh pcl 'cd ~/Others/tmls-rs && git pull && time docker build -t tmls-rs-build:latest -f tooling/docker/Dockerfile.build tooling/docker'`.
      Record the build time in the commit body of the next commit or in `tooling/docker/README.md`.

## B2 — gpui feasibility probe (de-risk before the slice)

gpui is the unknown: git-only dependency, heavy dep tree, Wayland/Vulkan/blade backend, may need a newer toolchain.

- [ ] 1. Pick a gpui commit: latest Zed release tag's commit. Record it and its licence (Apache-2.0) in `docs/licences.md`.
- [ ] 2. `spikes/s2-gpui` hello window: one `div` with text + a keystroke counter. `[dependencies] gpui = { git = "https://github.com/zed-industries/zed", rev = "<commit>" }`.
- [ ] 3. Docker build on PC (ask). Each missing system lib → add to `Dockerfile.build`, rebuild, retry. Record every lib and
      every workaround (patch, feature flag, toolchain pin) in `docs/spikes/S2-gpui-probe.md`. **Time-box 2 h of build
      fighting**; past that, stop and report to the user with the blocker — the S2 decision may then be "wgpu by default".
- [ ] 4. Host check: `ldd target-docker/release/s2-gpui | grep "not found"` empty. User runs the hello window at the PC (ask):
      opens, typing increments counter, Wayland (not XWayland: check `WAYLAND_DISPLAY` + `xlsclients` empty), quits clean.
- [ ] 5. Find out and write down in the probe doc: how to timestamp presentation in gpui (`on_next_frame`? a present
      callback? frame-callback only?) → which `present_kind` B4 will log; the IME input-handler API; clipboard API;
      whether a custom `Element` can paint a cell grid efficiently (text runs vs. glyph atlas access).
- [ ] 6. Commit `s2-gpui: feasibility probe (pinned gpui, hello window, deps)`.

## B3 — `s2-bench` driver + analysis (S2 plan Task 5)

Independent of A — works against the contract log schema and A1's golden log.

- `spikes/s2-bench/run.sh <wgpu|gpui> <quiet|flood> <hz-label> [scale-label]` (PC side): records environment (git hash,
  `kscreen-doctor -o`, `kwin_wayland --version`, `plasmashell --version`, `nvidia-smi` driver + pstate, `game-mode status`
  must be off → else abort) into `<dir>/env.txt`; starts the app with `--bench <dir> --photon --load-label <load>`;
  `flood` → `--cmd <path>/s0 flood --bytes-per-sec 500000`; prints "focus the window, hands off"; after 5 s sends a
  probe key via `ydotool` and aborts unless a `key` line appears within 3 s (K1 pattern); then 2000 plain keys with
  `ydotool type --key-delay 20`; then 60 s idle sampling; then closes the app.
- `spikes/s2-bench/analyze.py <dir>…` (archbox side, stdlib only): one Markdown row per run — app | load | Hz | scale |
  keys | key→write | write→output | output→submit | submit→present | **key→present p50/p95/p99** | present_kind | frames |
  dropped (gaps > 1.5 × frame time) | CPU % typing/flood/idle | RSS | GPU util/W. Missing keys (sent − logged) reported.
- `spikes/s2-bench/README.md`: how to run; photon method (phone 240 fps, count frames from key LED/sound to square flip,
  10 samples per app at max Hz → `docs/spikes/S2-results.md` by hand).

Steps:
- [ ] 1. Tests: `python3 -m unittest spikes/s2-bench/test_analyze.py` — synthetic logs (attribution rule, percentiles,
      dropped frames, missing keys); once A1 lands, assert `analyze.py` matches A1's `golden-report.json`.
- [ ] 2. `shellcheck spikes/s2-bench/run.sh` clean; dry-run mode `--dry-run` prints the commands without running them.
- [ ] 3. Commit `s2-bench: ydotool driver, sampler, analysis`.

## B4 — `s2-gpui` interaction slice (S2 plan Task 4)

Needs: A1 merged to `main` → rebase `spike/s2-gpui`. Same slice and CLI as `s2-wgpu` (contract): terminal grid as a custom
`Element` fed by `s2-core::Frame`, sidebar list, one editable text field (gpui has none built in — write a minimal one;
record the cost), focus traversal, context menu, IME through gpui's input handler, clipboard, font zoom, HiDPI.
Latency marks with the `present_kind` found in B2.

- [ ] 1. Tests for the pure logic gpui allows headless (grid → text-run layout, text-field edit ops); otherwise rely on
      `s2-core` tests and say so.
- [ ] 2. Docker build on PC (ask) → `ldd` clean → **user smoke run** (ask): `target-docker/release/s2-gpui --bench ~/s2-logs/gpui-smoke`.
      Copy log to `/data/scratch/s2/gpui-smoke/`, run `analyze.py`, iterate until frames arrive and p50 < 1 frame.
- [ ] 3. **User checklist** (same as A3: fcitx5 pinyin, wide-char copy, paste into field, monitor move, 125 % scaling) →
      `docs/spikes/S2-checklist-gpui.md`, plus implementation cost (LOC, hours, upstream workarounds).
- [ ] 4. Commit `s2-gpui: interaction slice in gpui`. One `PROGRESS.md` entry for Track B. Tell the user **"Track B ready for review"**.

## Also on this track (Claude only, not for Codex)

- Deferred **full review of S1b round 2** (`2178799..258cec1`, merged in `cfd0164`): `carry.rs` wrap carry + resize,
  `handoff.rs`, test coverage of the claims. Save `docs/reviews/2026-10-08-claude-s1b-r2.md`. If clean, drop "pending full
  Claude review" from PLAN §6 S1b; if not, write a fix prompt for Codex.
- Review + merge of A1 (sync point 2) and of B1 (so Track A gets the image).

## Done when

B1 on `main` and the PC image rebuilt; probe doc answers the 4 gpui questions; `analyze.py` tests green incl. the golden
log; gpui slice smoke + checklist done — or B2 stopped at the time-box with a written blocker report.
