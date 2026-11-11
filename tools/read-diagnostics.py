# /// script
# requires-python = ">=3.10"
# dependencies = ["pyusb>=1.3,<2"]
# ///
"""Read the optional Mosaico USB diagnostic log without resetting the receiver."""
import argparse
import json
from pathlib import Path
import struct
import usb.core

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--serial", default="vial:f64c2b3c:d08b40")
parser.add_argument("--output", type=Path)
parser.add_argument("--presents", action="store_true")
parser.add_argument("--accelerator", action="store_true")
args = parser.parse_args()
devices = [d for d in usb.core.find(find_all=True, idVendor=0x4c4b, idProduct=0x4644) if d.serial_number == args.serial]
if len(devices) != 1:
    parser.error(f"expected one matching receiver, found {len(devices)}")
device = devices[0]
status = bytes(device.ctrl_transfer(0xc0, 0x74, 0x4d53, 0, 28, timeout=2000))
magic, length, panics, stage, frames, internal_free, gui_free = struct.unpack("<4s6I", status)
assert magic == b"QJD1", magic
assert length <= 8192, length
log = bytearray()
for offset in range(0, length, 128):
    log.extend(device.ctrl_transfer(0xc0, 0x74, 0x4d54, offset, min(128, length-offset), timeout=2000))
result = {"panic_core_mask": panics, "ui_stage": stage, "frames": frames, "internal_free": internal_free,
          "gui_free": gui_free, "log": log.decode("utf8", errors="replace")}
if args.presents:
    if "Full-frame benchmark complete" not in result["log"]:
        parser.error("frame benchmark is still running; read presents after completion")
    count = struct.unpack("<I", bytes(device.ctrl_transfer(0xc0, 0x74, 0x4d5a, 0xffff, 4, timeout=2000)))[0]
    assert count <= 2048
    fields = ("scenario", "render_us", "transfer_us", "total_us", "period_us", "dirty_pixels", "state_changed", "elapsed_us", "state_us", "js_us", "native_us", "damage_us", "raster_us", "commit_us")
    result["presents"] = []
    for index in range(count):
        data = bytes(device.ctrl_transfer(0xc0, 0x74, 0x4d5a, index, 56, timeout=2000))
        assert len(data) in (32, 56), len(data)
        result["presents"].append(dict(zip(fields, struct.unpack(f"<{len(data) // 4}I", data))))
if args.accelerator:
    count = struct.unpack("<I", bytes(device.ctrl_transfer(0xc0, 0x74, 0x4d5b, 0xffff, 4, timeout=2000)))[0]
    assert count <= 32
    names = ("width", "height", "cpu_us", "hardware_us", "setup_us", "wait_us", "finish_us", "mismatches", "error", "samples", "actual", "expected", "kind")
    result["accelerator"] = []
    for i in range(count):
        data = bytes(device.ctrl_transfer(0xc0, 0x74, 0x4d5b, i, 52, timeout=2000))
        assert len(data) in (40, 48, 52)
        result["accelerator"].append(dict(zip(names, struct.unpack(f"<{len(data) // 4}I", data))))
if args.output:
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2)+"\n")
print(json.dumps({k:v for k,v in result.items() if k not in ("log", "presents")}))
print(result["log"])
