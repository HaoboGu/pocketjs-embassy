"""Summarize completed DMA updates, excluding renders with no changed pixels."""
import argparse
import json
import math
from pathlib import Path
import statistics

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("capture", type=Path)
parser.add_argument("--output", type=Path)
parser.add_argument("--scenario-offset", type=int, default=0)
args = parser.parse_args()
capture = json.loads(args.capture.read_text())
assert capture["panic_core_mask"] == 0
assert "Full-frame benchmark complete" in capture["log"], "benchmark is still running"
scenarios = ("continuous_updates", "charging", "typing_80ms", "layer_modifier_240ms")
summary = {}
for scenario, name in enumerate(scenarios):
    rows = [row for row in capture["presents"] if row["scenario"] == scenario + args.scenario_offset]
    assert len(rows) in (48, 192), (name, len(rows))
    drawn = [row for row in rows if row["dirty_pixels"]]
    intervals = []
    previous = 0
    for row in drawn:
        intervals.append(row["elapsed_us"] - previous)
        previous = row["elapsed_us"]
    elapsed = rows[-1]["elapsed_us"]
    stats = {
        "samples": len(rows),
        "completed_frames": len(drawn),
        "elapsed_s": elapsed / 1_000_000,
        "render_wall_percent": round(sum(row["render_us"] for row in rows) * 100 / elapsed, 2),
        "work_and_transfer_wall_percent": round(sum(row["total_us"] for row in rows) * 100 / elapsed, 2),
        "completed_fps": round(len(drawn) * 1_000_000 / elapsed, 3),
        "state_updates_per_second": round(sum(row["state_changed"] for row in rows) * 1_000_000 / elapsed, 3),
        "frames_over_33_333_us": sum(row["total_us"] > 33_333 for row in drawn),
    }
    for key, values in (
        ("render_ms", [row["render_us"] for row in drawn]),
        ("transfer_ms", [row["transfer_us"] for row in drawn]),
        ("total_ms", [row["total_us"] for row in drawn]),
        ("completion_interval_ms", intervals),
    ):
        data = sorted(values)
        stats[key] = {label: round(value / 1000, 3) for label, value in {
            "mean": statistics.mean(data), "p50": statistics.median(data),
            "p95": data[math.ceil(len(data) * .95) - 1], "max": max(data),
        }.items()}
    for field in ("state_us", "js_us", "native_us", "damage_us", "raster_us", "commit_us"):
        if field in drawn[0]:
            data = sorted(row[field] / 1000 for row in drawn)
            stats[field.replace("_us", "_ms")] = {
                "mean": round(statistics.mean(data), 3), "p50": round(statistics.median(data), 3),
                "p95": data[math.ceil(len(data) * .95) - 1], "max": max(data),
            }
    summary[name] = stats
output = json.dumps(summary, indent=2) + "\n"
if args.output:
    args.output.write_text(output)
print(output, end="")
