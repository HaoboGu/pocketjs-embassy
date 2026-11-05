# /// script
# requires-python = ">=3.10"
# dependencies = ["pyusb>=1.3,<2"]
# ///
"""Sample the optional Mosaico display profiler without injecting input."""
import argparse
import json
from pathlib import Path
import statistics
import struct
import time
import usb.core

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--serial", default="vial:f64c2b3c:d08b40")
parser.add_argument("--seconds", type=float, default=20)
parser.add_argument("--output", type=Path, required=True)
parser.add_argument("--wait-input", action="store_true", help="wait for at least four state updates within one second")
args = parser.parse_args()
if not 1 <= args.seconds <= 300:
    parser.error("seconds must be between 1 and 300")
devices = [d for d in usb.core.find(find_all=True, idVendor=0x4c4b, idProduct=0x4644) if d.serial_number == args.serial]
if len(devices) != 1:
    parser.error(f"expected one matching receiver, found {len(devices)}")
device = devices[0]
if args.wait_input:
    from collections import deque
    recent = deque()
    deadline = time.monotonic() + 180
    print("Waiting for keyboard activity...", flush=True)
    while True:
        now = time.monotonic()
        revision = struct.unpack("<4I", bytes(device.ctrl_transfer(0xc0, 0x74, 0x4d56, 0, 16, timeout=2000)))[0]
        recent.append((now, revision))
        while recent and recent[0][0] < now - 1:
            recent.popleft()
        if ((revision - recent[0][1]) & 0xffffffff) >= 4:
            break
        if now >= deadline:
            parser.error("no sustained keyboard activity detected")
        time.sleep(0.05)
    print("Recording keyboard activity...", flush=True)
rows = []
start = time.monotonic()
while time.monotonic() - start < args.seconds:
    frame = struct.unpack("<10I", bytes(device.ctrl_transfer(0xc0, 0x74, 0x4d55, 0, 40, timeout=2000)))
    pipeline = struct.unpack("<4I", bytes(device.ctrl_transfer(0xc0, 0x74, 0x4d56, 0, 16, timeout=2000)))
    rows.append(dict(zip(("uptime_ms", "frames", "input_revision", "applied_revision", "snapshot_age_us", "apply_latency_us"), frame[:2] + pipeline)))
    time.sleep(0.05)
first, last = rows[0], rows[-1]
elapsed_ms = (last["uptime_ms"] - first["uptime_ms"]) & 0xffffffff
presented = (last["frames"] - first["frames"]) & 0xffffffff
updates = {r["applied_revision"]: r for r in rows if r["applied_revision"] != first["applied_revision"]}
summary = {"elapsed_ms": elapsed_ms, "presented_frames": presented,
    "presented_fps_including_idle": round(presented * 1000 / elapsed_ms, 2),
    "observed_applied_states": len(updates),
    "max_unapplied_revisions": max((r["input_revision"] - r["applied_revision"]) & 0xffffffff for r in rows)}
for field in ("snapshot_age_us", "apply_latency_us"):
    values = sorted(r[field] / 1000 for r in updates.values())
    if values:
        summary[field.replace("_us", "_ms")] = {"median": statistics.median(values), "p95": values[min(len(values) - 1, int(len(values) * 0.95))], "max": max(values)}
args.output.parent.mkdir(parents=True, exist_ok=True)
args.output.write_text(json.dumps({"summary": summary, "samples": rows}, indent=2) + "\n")
print(json.dumps(summary, indent=2))
