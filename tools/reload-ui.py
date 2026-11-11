# /// script
# requires-python = ">=3.10"
# dependencies = ["pyusb>=1.3,<2"]
# ///
"""Hot-reload a prebuilt PocketJS bundle over USB without resetting the receiver."""
import argparse
import errno
from dataclasses import dataclass
import json
from pathlib import Path
import struct
import sys
import time
import zlib
import usb.core
import usb.util

FIELDS = ("magic", "protocol", "abi", "chunk_size", "max_bytes", "phase", "received",
          "total", "generation", "active_crc32", "error", "uptime_ms")

@dataclass(frozen=True)
class Bundle:
    manifest: Path
    javascript_path: Path
    assets_path: Path
    abi: int
    viewport: tuple[int, int]
    javascript: bytes
    assets: bytes
    published: tuple[int, int, int]


def manifest_stamp(path):
    info = path.stat()
    return info.st_mtime_ns, info.st_size, info.st_ino


def load_bundle(source):
    source = Path(source).expanduser().absolute()
    manifest = source / "app.meta.json" if source.is_dir() else source
    published = manifest_stamp(manifest)
    meta = json.loads(manifest.read_text(encoding="utf8"))
    if not isinstance(meta, dict):
        raise ValueError(f"{manifest}: expected a JSON object")
    abi = meta.get("abi")
    if type(abi) is not int or not 0 < abi <= 0xffffffff:
        raise ValueError(f"{manifest}: abi must be a positive 32-bit integer")
    viewport = meta.get("viewport")
    if (not isinstance(viewport, list) or len(viewport) != 2
            or any(type(size) is not int or not 0 < size <= 0xffffffff for size in viewport)):
        raise ValueError(f"{manifest}: viewport must contain two positive integer dimensions")
    paths = []
    for field, default in (("javascript", "app.js"), ("assets", "app.pak")):
        filename = meta.get(field, default)
        if not isinstance(filename, str) or not filename:
            raise ValueError(f"{manifest}: {field} must be a nonempty file path")
        paths.append(manifest.parent / filename)
    javascript, assets = (path.read_bytes() for path in paths)
    if not javascript:
        raise ValueError(f"{paths[0]}: JavaScript artifact is empty")
    if manifest_stamp(manifest) != published:
        raise ValueError(f"{manifest}: changed while reading; publish the manifest after the artifacts")
    return Bundle(manifest, paths[0], paths[1], abi, tuple(viewport), javascript, assets, published)


def watch_bundles(bundle):
    # The build publishes this manifest last; never upload on a partial asset write.
    published = bundle.published
    last_error = None
    while True:
        time.sleep(0.3)
        try:
            current = manifest_stamp(bundle.manifest)
            if current == published:
                continue
            published = current
            candidate = load_bundle(bundle.manifest)
        except (OSError, ValueError) as error:
            message = f"Bundle not ready: {error}"
            if message != last_error:
                print(message, file=sys.stderr, flush=True)
            last_error = message
            continue
        last_error = None
        yield candidate


def status(device):
    data = bytes(device.ctrl_transfer(0xc0, 0x75, 0x4d53, 0, 48, timeout=2000))
    if len(data) != 48 or data[:4] != b"MUI1":
        raise RuntimeError("This firmware does not expose the UI reload protocol")
    return dict(zip(FIELDS[1:], struct.unpack("<11I", data[4:])))

def wait_for(device, predicate, timeout=15):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        current = status(device)
        if current["phase"] == 4:
            message = bytes(device.ctrl_transfer(0xc0, 0x75, 0x4d53, 1, 128, timeout=2000)).decode("utf8", errors="replace")
            raise RuntimeError(f"UI reload rejected ({current['error']}): {message}")
        if predicate(current):
            return current
        time.sleep(0.05 if current["phase"] == 2 else 0.002)
    raise TimeoutError(f"UI reload did not progress: {status(device)}")

def upload(device, bundle):
    javascript, assets = bundle.javascript, bundle.assets
    before = status(device)
    if before["protocol"] != 2:
        raise RuntimeError("Receiver does not support bulk UI reload protocol 2")
    if bundle.abi != before["abi"] or bundle.viewport != (480, 480):
        raise ValueError("Bundle ABI or viewport does not match the receiver")
    payload = javascript + assets
    if len(payload) > before["max_bytes"]:
        raise ValueError("Bundle exceeds the receiver's upload budget")
    checksum = zlib.crc32(payload)
    header = struct.pack("<4s4I", b"MUI1", bundle.abi, len(javascript), len(assets), checksum)
    start = time.monotonic()
    # Abort any abandoned upload before starting a fresh transaction.
    device.ctrl_transfer(0x40, 0x75, 0x4d53, 3, b"", timeout=2000)
    deadline = time.monotonic() + 5
    while status(device)["phase"] != 0:
        if time.monotonic() > deadline: raise TimeoutError("Could not clear the previous upload")
        time.sleep(0.005)
    while True:
        try:
            device.ctrl_transfer(0x40, 0x75, 0x4d53, 0, header, timeout=2000)
            break
        except usb.core.USBError as error:
            # An already-idle status can precede consumption of the abort packet.
            # A stalled begin was rejected by the one-packet queue and is safe to retry.
            if error.errno != errno.EPIPE or time.monotonic() >= deadline:
                raise
            time.sleep(0.005)
    wait_for(device, lambda s: s["phase"] == 1 and s["received"] == 0 and s["total"] == len(payload))
    interface = usb.util.find_descriptor(device.get_active_configuration(),
        bInterfaceClass=0xff, bInterfaceSubClass=0x4d, bInterfaceProtocol=1)
    if interface is None: raise RuntimeError("The firmware has no UI bulk upload interface")
    endpoint = usb.util.find_descriptor(interface,
        custom_match=lambda ep: usb.util.endpoint_direction(ep.bEndpointAddress) == usb.util.ENDPOINT_OUT)
    if endpoint is None: raise RuntimeError("The UI upload endpoint is missing")
    chunk_size = before["chunk_size"]
    if chunk_size + 4 != endpoint.wMaxPacketSize:
        raise RuntimeError("Bundle packet framing does not match the USB endpoint")
    usb.util.claim_interface(device, interface.bInterfaceNumber)
    offset = 0
    reported = 0
    try:
        while offset < len(payload):
            batch = bytearray()
            for _ in range(32):
                if offset == len(payload): break
                chunk = payload[offset:offset + chunk_size]
                batch.extend(struct.pack("<I", offset))
                batch.extend(chunk)
                offset += len(chunk)
            device.write(endpoint.bEndpointAddress, batch, timeout=5000)
            wait_for(device, lambda s: s["received"] == offset)
            progress = offset * 100 // len(payload)
            if progress >= reported + 20:
                print(f"Uploaded {progress}%", flush=True)
                reported = progress
    finally:
        usb.util.release_interface(device, interface.bInterfaceNumber)
    uploaded_at = time.monotonic()
    device.ctrl_transfer(0x40, 0x75, 0x4d53, 2, b"", timeout=2000)
    after = wait_for(device, lambda s: s["phase"] == 3
                     and s["generation"] == (before["generation"] + 1) % 2**32
                     and s["active_crc32"] == checksum, timeout=60)
    if after["uptime_ms"] < before["uptime_ms"]:
        raise RuntimeError("Receiver uptime reset during the upload")
    return {"bundle": str(bundle.manifest.parent), "manifest": str(bundle.manifest), "bytes": len(payload), "elapsed_s": round(time.monotonic() - start, 3),
            "transport": "usb_bulk", "batch_packets": 32,
            "transfer_s": round(uploaded_at - start, 3), "apply_s": round(time.monotonic() - uploaded_at, 3), "before": before, "after": after}

def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("path", nargs="?", type=Path, help="Prebuilt bundle directory or JSON manifest")
    parser.add_argument("--bundle", type=Path, help="Alias for the positional bundle path")
    parser.add_argument("--serial", help="Required if several Mosaico receivers are connected")
    parser.add_argument("--status", action="store_true", help="Read receiver status without uploading")
    parser.add_argument("--check", action="store_true", help="Inspect bundle files and metadata without accessing USB")
    parser.add_argument("--watch", action="store_true", help="Reload when the build publishes an updated manifest")
    parser.add_argument("--output", type=Path, help="Save the last successful report as JSON")
    args = parser.parse_args(argv)
    if args.path is not None and args.bundle is not None:
        parser.error("specify the bundle once, as a path or with --bundle")
    source = args.path if args.path is not None else args.bundle
    if args.status:
        if source is not None or args.watch or args.check:
            parser.error("--status cannot be combined with a bundle, --watch, or --check")
    elif source is None:
        parser.error("provide a prebuilt bundle directory or JSON manifest")
    if args.watch and args.check:
        parser.error("--check inspects once; use --watch to upload subsequent builds")

    def show(report):
        text = json.dumps(report, indent=2)
        if args.output:
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(text + "\n")
        print(text, flush=True)

    # Read and validate the requested artifact before opening a device.
    bundle = load_bundle(source) if source is not None else None
    if args.check:
        show({"manifest": str(bundle.manifest), "javascript": str(bundle.javascript_path),
              "assets": str(bundle.assets_path), "abi": bundle.abi, "viewport": bundle.viewport,
              "javascript_bytes": len(bundle.javascript), "assets_bytes": len(bundle.assets),
              "bytes": len(bundle.javascript) + len(bundle.assets), "device_checked": False})
        return
    devices = [device for device in usb.core.find(find_all=True, idVendor=0x4c4b, idProduct=0x4644)
               if args.serial is None or device.serial_number == args.serial]
    if len(devices) != 1:
        parser.error(f"found {len(devices)} receivers; connect one or specify --serial")
    device = devices[0]
    if args.status:
        show(status(device))
        return
    show(upload(device, bundle))
    if args.watch:
        print(f"Watching {bundle.manifest}; publish this file last. Ctrl-C stops watching.", flush=True)
        applied = bundle
        for candidate in watch_bundles(bundle):
            if (candidate.abi, candidate.viewport, candidate.javascript, candidate.assets) == (
                    applied.abi, applied.viewport, applied.javascript, applied.assets):
                continue
            try:
                show(upload(device, candidate))
                applied = candidate
            except (OSError, ValueError, RuntimeError) as error:
                print(f"Reload failed: {error}", file=sys.stderr, flush=True)


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        pass
    except (OSError, ValueError, RuntimeError) as error:
        print(f"Reload failed: {error}", file=sys.stderr)
        sys.exit(1)
