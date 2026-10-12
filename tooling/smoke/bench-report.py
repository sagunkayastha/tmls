#!/usr/bin/env python3
"""Reads the runs bench.sh wrote and prints their numbers.

    python3 -I tooling/smoke/bench-report.py DIR…            one line of numbers per run
    python3 -I tooling/smoke/bench-report.py --json DIR…     the same as JSON

Each DIR holds `bench/latency.jsonl` (the app's `--bench` log) and `marks.json` (bench.sh).
All times are CLOCK_MONOTONIC ns, in the log and in the marks.

What is counted, per run:
  keys          `key` lines whose t_key_ns is between the marks t_type_start_ns and
                t_type_end_ns; `unframed` of them are in no `frame` line's `keys`
  median, p95   of t_present_ns − t_key_ns in ms, the frame being the first `frame` line that
                has the key's id in `keys` (p95: nearest rank)
  idle frames   `frame` lines with t_present_ns between the marks t_idle_start_ns and
                t_idle_end_ns (the app writes one for each frame of the logged tab it presents)
  idle cpu      utime+stime ticks of the app over that window (/proc/PID/stat), as ms
  idle commits  wl_surface commits in that window, in runs with BENCH_WLDEBUG=1 (else "-")
  rss           VmRSS from /proc/PID/status: `start` 3 s after the window opened, `open` with
                the panel showing and before typing, `closed` 3 s after Alt+Shift+X;
                `max` is the largest of the `sample` lines' rss_kb (one every 500 ms) between
                the marks t_open_ns and t_close_ns and of the script's own readings there;
                `hwm` is VmHWM (the peak since start) when the idle window ended
  first frame   mark t_window_ns − mark t_launch_ns: from just before exec of the app to sway's
                event for the new window (its first committed buffer)
"""
import json
import os
import sys


def percentile(sorted_values, p):
    """Nearest rank."""
    if not sorted_values:
        return None
    rank = max(1, -(-len(sorted_values) * p // 100))
    return sorted_values[int(rank) - 1]


def read_run(directory):
    marks = json.load(open(os.path.join(directory, "marks.json")))
    keys, frame_of, frames, samples, meta = {}, {}, [], [], None
    for line in open(os.path.join(directory, "bench", "latency.jsonl")):
        try:
            ev = json.loads(line)
        except ValueError:
            continue
        kind = ev.get("ev")
        if kind == "key":
            keys[ev["id"]] = ev["t_key_ns"]
        elif kind == "frame":
            frames.append(ev)
            for key in ev["keys"]:
                frame_of.setdefault(key, ev)
        elif kind == "sample":
            samples.append(ev)
        elif kind == "meta":
            meta = ev

    typed = [k for k, t in keys.items() if marks["t_type_start_ns"] <= t <= marks["t_type_end_ns"]]
    latencies = sorted((frame_of[k]["t_present_ns"] - keys[k]) / 1e6 for k in typed if k in frame_of)
    typed_ids = set(typed)
    typing_frames = [f for f in frames if typed_ids.intersection(f["keys"])]
    idle = (marks["t_idle_start_ns"], marks["t_idle_end_ns"])
    idle_frames = [f for f in frames if idle[0] <= f["t_present_ns"] <= idle[1]]
    idle_samples = [s for s in samples if idle[0] <= s["t_ns"] <= idle[1]]
    open_samples = [s["rss_kb"] for s in samples if marks["t_open_ns"] <= s["t_ns"] <= marks["t_close_ns"]]
    own = [marks[name] for name in ("rss_open_kb", "rss_typed_kb", "rss_idle_start_kb", "rss_idle_end_kb")]
    tick_ms = 1000.0 / marks.get("clk_tck", 100)
    mb = lambda kb: kb / 1024.0
    commits = None
    if "commits_idle_end" in marks:
        commits = marks["commits_idle_end"] - marks["commits_idle_start"]
    return {
        "dir": directory,
        "state": marks.get("state"),
        "bin": marks.get("bin_sha"),
        "git": (meta or {}).get("git"),
        "failed_steps": marks.get("failed"),
        "keys": len(typed),
        "unframed": len([k for k in typed if k not in frame_of]),
        "typing_frames": len(typing_frames),
        "median_ms": percentile(latencies, 50),
        "p95_ms": percentile(latencies, 95),
        "max_ms": latencies[-1] if latencies else None,
        "idle_frames": len(idle_frames),
        "idle_cpu_ms": (marks["cpu_idle_end_ticks"] - marks["cpu_idle_start_ticks"]) * tick_ms,
        "idle_cpu_pct_samples": max((s["cpu_pct"] for s in idle_samples), default=None),
        "idle_commits": commits,
        "rss_start_mb": mb(marks["rss_start_kb"]),
        "rss_open_mb": mb(marks["rss_open_kb"]),
        "rss_max_mb": mb(max(open_samples + own)),
        "hwm_mb": mb(marks["hwm_idle_end_kb"]),
        "rss_closed_mb": mb(marks["rss_closed_kb"]),
        "first_frame_ms": (marks["t_window_ns"] - marks["t_launch_ns"]) / 1e6,
        "cols": (marks.get("cols_closed"), marks.get("cols_open")),
        "load": (marks.get("load_before"), marks.get("load_after")),
    }


def main():
    args = sys.argv[1:]
    as_json = args[:1] == ["--json"]
    if as_json:
        args = args[1:]
    if not args:
        sys.exit(__doc__)
    runs = [read_run(d.rstrip("/")) for d in args]
    if as_json:
        json.dump(runs, sys.stdout, indent=1)
        print()
        return
    head = ("run", "keys", "unfr", "median", "p95", "max", "idle fr", "idle cpu", "commits",
            "rss start", "open", "max", "hwm", "closed", "first fr", "load")
    print("%-16s %4s %4s %7s %7s %7s %7s %8s %7s %9s %7s %7s %7s %7s %8s  %s" % head)
    for r in runs:
        print("%-16s %4d %4d %7.2f %7.2f %7.2f %7d %6.0fms %7s %9.1f %7.1f %7.1f %7.1f %7.1f %8.1f  %s/%s%s" % (
            os.path.basename(r["dir"]), r["keys"], r["unframed"],
            r["median_ms"] or 0, r["p95_ms"] or 0, r["max_ms"] or 0,
            r["idle_frames"], r["idle_cpu_ms"],
            "-" if r["idle_commits"] is None else r["idle_commits"],
            r["rss_start_mb"], r["rss_open_mb"], r["rss_max_mb"], r["hwm_mb"], r["rss_closed_mb"],
            r["first_frame_ms"], r["load"][0], r["load"][1],
            "" if not r["failed_steps"] else "  (%s steps FAILED)" % r["failed_steps"]))


if __name__ == "__main__":
    main()
