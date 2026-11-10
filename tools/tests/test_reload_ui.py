"""Host-only checks for artifact selection, publication and the USB wire payload."""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import struct
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import zlib

spec = importlib.util.spec_from_file_location(
    "mosaico_reload_ui", Path(__file__).resolve().parents[1] / "reload-ui.py"
)
reload_ui = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = reload_ui
spec.loader.exec_module(reload_ui)


class Receiver:
    """Decode the documented protocol; no USB devices are opened."""
    def __init__(self):
        self.phase = 3
        self.received = bytearray()
        self.total = 0
        self.generation = 9
        self.active_crc = 123
        self.clock = 100
        self.max_bytes = 2 * 1024 * 1024
        self.writes = []
        self.header = None

    def ctrl_transfer(self, direction, request, magic, operation, data, timeout):
        assert request == 0x75 and magic == 0x4d53
        self.clock += 1
        if direction == 0xc0:
            assert operation == 0 and data == 48
            return struct.pack("<4s11I", b"MUI1", 2, 12, 508, self.max_bytes,
                               self.phase, len(self.received), self.total,
                               self.generation, self.active_crc, 0, self.clock)
        assert direction == 0x40
        self.writes.append(operation)
        if operation == 3:
            assert not data
            self.phase = 0
            self.received.clear()
            self.total = 0
        elif operation == 0:
            self.header = struct.unpack("<4s4I", data)
            package_magic, abi, js_size, pak_size, _ = self.header
            assert package_magic == b"MUI1" and abi == 12
            self.total = js_size + pak_size
            self.phase = 1
        elif operation == 2:
            assert not data and len(self.received) == self.total
            assert zlib.crc32(self.received) == self.header[4]
            self.active_crc = self.header[4]
            self.generation += 1
            self.phase = 3
        else:
            raise AssertionError(f"Unexpected control operation {operation}")
        return len(data)

    def get_active_configuration(self):
        return object()

    def write(self, address, data, timeout):
        assert address == 2 and self.phase == 1
        for start in range(0, len(data), 512):
            packet = data[start:start + 512]
            offset = struct.unpack("<I", packet[:4])[0]
            assert offset == len(self.received)
            self.received.extend(packet[4:])
        return len(data)


class ReloadTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="external-pocketjs-build-")
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name)
        self.revision = 0

    def publish(self, manifest, metadata):
        manifest.write_text(json.dumps(metadata))
        self.revision += 1
        stamp = 1_000_000_000_000 + self.revision * 1_000_000
        os.utime(manifest, ns=(stamp, stamp))

    def bundle(self, custom=False):
        js = self.directory / ("dashboard.js" if custom else "app.js")
        pak = self.directory / ("resources/dashboard.pak" if custom else "app.pak")
        pak.parent.mkdir(exist_ok=True)
        js.write_bytes(b"globalThis.frame = () => {}; // external application")
        pak.write_bytes(bytes(range(256)) * 4)
        manifest = self.directory / ("dashboard.json" if custom else "app.meta.json")
        metadata = {"abi": 12, "viewport": [480, 480]}
        if custom:
            metadata.update(javascript=js.name, assets="resources/dashboard.pak", name="Unregistered UI")
        self.publish(manifest, metadata)
        return manifest, js, pak, metadata

    def test_external_manifest_selects_custom_artifact_names(self):
        manifest, js, pak, _ = self.bundle(custom=True)
        bundle = reload_ui.load_bundle(manifest)
        self.assertEqual(bundle.javascript_path, js)
        self.assertEqual(bundle.assets_path, pak)
        self.assertEqual(bundle.javascript, js.read_bytes())
        self.assertEqual(bundle.assets, pak.read_bytes())

    def test_directory_and_legacy_flag_inspect_without_usb(self):
        self.bundle()
        for arguments in ([str(self.directory), "--check"], ["--bundle", str(self.directory), "--check"]):
            with self.subTest(arguments=arguments), patch.object(reload_ui.usb.core, "find") as discover:
                output = io.StringIO()
                with contextlib.redirect_stdout(output):
                    reload_ui.main(arguments)
                report = json.loads(output.getvalue())
                self.assertEqual(report["abi"], 12)
                self.assertFalse(report["device_checked"])
                discover.assert_not_called()

    def test_invalid_artifacts_fail_before_usb_discovery(self):
        manifest, js, pak, metadata = self.bundle()
        invalid = [dict(metadata, abi="12"), dict(metadata, viewport=[480]),
                   dict(metadata, assets=None), dict(metadata, javascript="missing.js")]
        for candidate in invalid:
            with self.subTest(candidate=candidate), patch.object(reload_ui.usb.core, "find") as discover:
                self.publish(manifest, candidate)
                with self.assertRaises((OSError, ValueError)):
                    reload_ui.main([str(self.directory)])
                discover.assert_not_called()
        self.publish(manifest, metadata)
        js.write_bytes(b"")
        with self.assertRaisesRegex(ValueError, "empty"):
            reload_ui.load_bundle(manifest)
        js.write_bytes(b"void 0")
        pak.unlink()
        with self.assertRaises(FileNotFoundError):
            reload_ui.load_bundle(manifest)

    def test_upload_sends_exact_external_bundle_without_theme_registration(self):
        manifest, js, pak, _ = self.bundle(custom=True)
        bundle = reload_ui.load_bundle(manifest)
        receiver = Receiver()
        interface = SimpleNamespace(bInterfaceNumber=7)
        endpoint = SimpleNamespace(bEndpointAddress=2, wMaxPacketSize=512)
        with patch.object(reload_ui.usb.util, "find_descriptor", side_effect=[interface, endpoint]), \
             patch.object(reload_ui.usb.util, "claim_interface") as claim, \
             patch.object(reload_ui.usb.util, "release_interface") as release, \
             contextlib.redirect_stdout(io.StringIO()):
            report = reload_ui.upload(receiver, bundle)
        self.assertEqual(receiver.received, js.read_bytes() + pak.read_bytes())
        self.assertEqual(receiver.header[2:4], (js.stat().st_size, pak.stat().st_size))
        self.assertEqual(receiver.writes, [3, 0, 2])
        self.assertEqual(report["manifest"], str(manifest))
        self.assertEqual(report["after"]["generation"], 10)
        claim.assert_called_once_with(receiver, 7)
        release.assert_called_once_with(receiver, 7)

    def test_incompatible_package_never_starts_an_upload(self):
        manifest, _, _, metadata = self.bundle()
        for candidate in (dict(metadata, abi=99), dict(metadata, viewport=[320, 240])):
            self.publish(manifest, candidate)
            receiver = Receiver()
            with self.assertRaisesRegex(ValueError, "does not match"):
                reload_ui.upload(receiver, reload_ui.load_bundle(manifest))
            self.assertEqual(receiver.writes, [])
        self.publish(manifest, metadata)
        receiver = Receiver()
        receiver.max_bytes = 1
        with self.assertRaisesRegex(ValueError, "budget"):
            reload_ui.upload(receiver, reload_ui.load_bundle(manifest))
        self.assertEqual(receiver.writes, [])

    def test_watch_waits_for_manifest_before_reading_changed_artifacts(self):
        manifest, js, pak, metadata = self.bundle()
        bundle = reload_ui.load_bundle(manifest)
        steps = []
        def build_step(_):
            steps.append(len(steps) + 1)
            if len(steps) == 1:
                js.write_bytes(b"new javascript")
            elif len(steps) == 2:
                pak.write_bytes(b"new assets")
            elif len(steps) == 3:
                self.publish(manifest, metadata)
            else:
                self.fail("Completed bundle was not detected")
        with patch.object(reload_ui.time, "sleep", side_effect=build_step):
            candidate = next(reload_ui.watch_bundles(bundle))
        self.assertEqual(steps, [1, 2, 3])
        self.assertEqual(candidate.javascript, b"new javascript")
        self.assertEqual(candidate.assets, b"new assets")

    def test_watch_keeps_changes_published_during_the_previous_upload(self):
        manifest, js, _, metadata = self.bundle()
        bundle = reload_ui.load_bundle(manifest)
        js.write_bytes(b"published while the previous upload was running")
        self.publish(manifest, metadata)
        with patch.object(reload_ui.time, "sleep") as sleep:
            candidate = next(reload_ui.watch_bundles(bundle))
        self.assertEqual(candidate.javascript, js.read_bytes())
        sleep.assert_called_once()

    def test_watch_recovers_after_invalid_manifest(self):
        manifest, js, _, metadata = self.bundle()
        bundle = reload_ui.load_bundle(manifest)
        steps = 0
        def build_step(_):
            nonlocal steps
            steps += 1
            if steps == 1:
                self.publish(manifest, dict(metadata, abi="incomplete build"))
            elif steps == 2:
                js.write_bytes(b"valid next build")
                self.publish(manifest, metadata)
            else:
                self.fail("Watcher did not recover")
        errors = io.StringIO()
        with patch.object(reload_ui.time, "sleep", side_effect=build_step), contextlib.redirect_stderr(errors):
            candidate = next(reload_ui.watch_bundles(bundle))
        self.assertEqual(steps, 2)
        self.assertEqual(candidate.javascript, b"valid next build")
        self.assertIn("Bundle not ready", errors.getvalue())

    def test_watch_skips_applied_payload_but_retries_failed_payload(self):
        manifest, js, pak, metadata = self.bundle()
        original_js = js.read_bytes()
        original_pak = pak.read_bytes()
        original = reload_ui.load_bundle(manifest)
        js.write_bytes(b"updated external UI")
        self.publish(manifest, dict(metadata, layout="any-label-is-allowed"))
        updated = reload_ui.load_bundle(manifest)
        js.write_bytes(original_js)
        pak.write_bytes(original_pak)
        self.publish(manifest, metadata)
        with patch.object(reload_ui.usb.core, "find", return_value=[object()]), \
             patch.object(reload_ui, "watch_bundles", return_value=iter([original, updated, updated, updated])), \
             patch.object(reload_ui, "upload", side_effect=[{}, RuntimeError("temporary failure"), {}]) as upload, \
             contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            reload_ui.main([str(self.directory), "--watch"])
        self.assertEqual(upload.call_count, 3)
        self.assertEqual(upload.call_args_list[0].args[1].javascript, original_js)
        self.assertEqual(upload.call_args_list[1].args[1], updated)
        self.assertEqual(upload.call_args_list[2].args[1], updated)

    def test_cli_requires_an_artifact_instead_of_selecting_a_default_theme(self):
        with patch.object(reload_ui.usb.core, "find") as discover, contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit) as failure:
                reload_ui.main([])
            self.assertEqual(failure.exception.code, 2)
            with self.assertRaises(SystemExit):
                reload_ui.main(["--layout", "github"])
            discover.assert_not_called()


if __name__ == "__main__":
    unittest.main()
