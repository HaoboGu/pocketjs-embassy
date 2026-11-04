# PocketJS + Embassy experiment

Run a Solid TSX interface through QuickJS-NG while keeping the Rust UI library
`no_std + alloc`. The application is compiled to JavaScript and a font/style
package; no MicroTS AOT code is generated or linked.

The shared library uses PocketJS's `Handle` and `CGuest` APIs. The host owns the
guest lifecycle and renders its UI with `pocket-raster`. This initial experiment
uses embedded bundles rather than the full application-package admission API.

## Try it on the host

Install stable Rust, Bun, Git, Python 3, and a C compiler. From this directory:

```sh
./tools/setup.sh
./tools/build-ui.sh
cargo test --release --locked
cargo run --release --locked --example embassy
cargo run --release --locked --example preview
python3 tools/preview.py
```

The Embassy example runs 60 JS-driven UI updates alongside a one-millisecond
heartbeat task, checks the native text after every update, then exits. The preview
writes `target/preview.png` with the counter set to 7. Host executables use `std`
for their executor, file output and test harness; the shared library remains
`no_std`.

## Build for ESP32-S31

```sh
./tools/setup.sh --s31
cd firmware/esp32s31
cargo build --release --locked
```

The ELF is `firmware/esp32s31/target/riscv32imafc-unknown-none-elf/release/pocketjs-embassy-s31`.
The setup script downloads an isolated Espressif C toolchain into `.deps`; it does
not replace system tools. The tested build host is macOS arm64.

The Mosaico firmware uses `esp-hal` and `esp-rtos` with Embassy. Core 0 runs a
heartbeat task. Core 1 owns QuickJS, the UI and the CO5300 display; it displays a
synthetic counter once per second and awaits `embassy-time` between updates.
Rust and C allocations share the GUI heap in PSRAM. The panel keeps a separate
framebuffer prefix, and RTOS allocations have an internal-memory heap.

The RISC-V firmware builds and links, but this repository's firmware has not yet
been flashed or measured on the board. RMK keyboard, USB and BLE integration is
not included in this standalone probe.

## Embassy integration rules

- Construct and drive `JsUi` inside one task on its owning executor. It is not
  `Send`; do not move a live QuickJS context between cores.
- Exchange keyboard state through an Embassy `Watch`, channel or signal before
  calling the guest. The demo injects `setKeyCount(...)` instead.
- JS evaluation and rendering are synchronous. Yield between updates. The
  interrupt-poll budget terminates runaway JS; it is not a wall-clock deadline
  and does not make GC or native rendering preemptible.
- The guest has a 4 MiB engine heap limit and a 128 KiB JS stack limit. The S31 UI
  executor has a 192 KiB stack, leaving space for Rust and driver frames. These
  settings still require hardware timing and stack-use validation.
- The embedded `Date` clock reports time since boot through `embassy-time`, not
  calendar time. This probe provides no filesystem or network JS bindings.

## Source and dependency pins

| Component | Source |
| --- | --- |
| UI | [`ui/app.tsx`](ui/app.tsx) and checked-in [`ui/generated`](ui/generated) assets |
| Rust runtime and C guest bindings | PocketJS `becc0e21b678384ec31efa2b4f8d0be3aeefad5f` |
| JS engine | QuickJS-NG v0.14.0, commit `3c051980ab7e783dfbfb1c70c014ce5e05ecf24c` |
| ESP support | esp-hal fork `163e6b62b5b3f1d1dc2cae10b6ccc68ed9e92fde` |
| C toolchain | Espressif `esp-16.1.0_20260609`, `rv32imafc` / `ilp32f` |

[`patches/quickjs-port.patch`](patches/quickjs-port.patch) makes four QuickJS local
variables match the `int32_t *` APIs on newlib. It also applies the immutable
ArrayBuffer checks used by PocketJS's ESP-IDF adapter, so JS cannot write into
the borrowed font/style package. The setup script applies this patch to the
pinned source. The target build uses esp-hal's stack protection implementation
and excludes newlib's duplicate startup object.

The CO5300 driver and window alignment code were copied from the existing RMK
Mosaico implementation at commit `10410241b`. This experiment is GPL-3.0-or-later;
upstream PocketJS, QuickJS, Solid and font notices are in [`licenses`](licenses).

Run `./tools/verify.sh` after setup to check formatting, linting, runtime behavior,
the host Embassy loop, the preview, and the final S31 firmware link. No verification
command flashes or resets hardware.
