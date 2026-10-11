#!/usr/bin/env python3
"""Writes golden-latency.jsonl and golden-report.json (run from this directory).

50 keys, 100 ms apart. For key i the stage times are
  key->write     (i % 5 + 1) * 10 us
  write->output  (i % 7 + 1) * 100 us if i % 3 == 0, else -(i % 7 + 1) us
  output->submit (i % 3 + 1) * 1 ms
  submit->present(i % 4 + 1) * 2 ms
with two exceptions: keys 10 and 11 land in one frame (key 11's), and key 49 is never
presented. Percentiles are nearest-rank: sorted[ceil(p/100 * n) - 1].

A negative write->output is real: the tty echo can be read before write() returns, so a frame's
t_output_ns may be a little earlier than the key's t_write_ns. Such a span counts as 0. Two keys
in three are like that here, so the median of write_to_output is 0 and an analysis that does not
clamp reports a negative p50.
The report here is computed independently of the Rust code that the test checks against it.
"""
import json
import math

US, MS = 1_000, 1_000_000
lines = [{"ev": "meta", "app": "golden", "git": "0000000", "profile": "release", "hz": 144.0,
          "scale": 1.25, "present_mode": "fifo", "load": "quiet"}]
keys, frames = {}, []
for i in range(50):
    t_key = 1_000 * MS + i * 100 * MS
    t_write = t_key + (i % 5 + 1) * 10 * US
    t_output = t_write + (i % 7 + 1) * 100 * US if i % 3 == 0 else t_write - (i % 7 + 1) * US
    t_submit = t_output + (i % 3 + 1) * MS
    t_present = t_submit + (i % 4 + 1) * 2 * MS
    keys[i] = (t_key, t_write)
    lines.append({"ev": "key", "id": i, "t_key_ns": t_key})
    lines.append({"ev": "write", "id": i, "t_write_ns": t_write})
    if i in (10, 49):
        continue
    frames.append({"ev": "frame", "seq": i, "t_output_ns": t_output, "t_submit_ns": t_submit,
                   "t_present_ns": t_present, "present_kind": "present_return",
                   "keys": [10, 11] if i == 11 else [i]})
    lines.append(frames[-1])
    if i % 10 == 0:
        lines.append({"ev": "sample", "t_ns": t_present, "cpu_pct": 3.5, "rss_kb": 81234, "fps": 10.0,
                      "gpu_util": None, "gpu_w": None})
# A frame that carries no key (output without typing).
lines.append({"ev": "frame", "seq": 50, "t_output_ns": 7_000 * MS, "t_submit_ns": 7_001 * MS,
              "t_present_ns": 7_003 * MS, "present_kind": "present_return", "keys": []})

stages = {name: [] for name in ("key_to_write", "write_to_output", "output_to_submit",
                                "submit_to_present", "key_to_present")}
for f in frames:
    for k in f["keys"]:
        t_key, t_write = keys[k]
        stages["key_to_write"].append(t_write - t_key)
        stages["write_to_output"].append(max(0, f["t_output_ns"] - t_write))
        stages["output_to_submit"].append(f["t_submit_ns"] - f["t_output_ns"])
        stages["submit_to_present"].append(f["t_present_ns"] - f["t_submit_ns"])
        stages["key_to_present"].append(f["t_present_ns"] - t_key)


def pcts(values):
    values = sorted(values)
    rank = lambda p: values[math.ceil(p / 100 * len(values)) - 1]
    return {"n": len(values), "p50": rank(50), "p95": rank(95), "p99": rank(99)}


with open("golden-latency.jsonl", "w") as out:
    for line in lines:
        out.write(json.dumps(line, separators=(",", ":")) + "\n")
with open("golden-report.json", "w") as out:
    json.dump({name: pcts(values) for name, values in stages.items()}, out, indent=2)
    out.write("\n")
