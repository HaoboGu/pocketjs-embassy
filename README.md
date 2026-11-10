# RMK Mosaico Dongle with QuickJS

A BLE-to-USB RMK receiver for ESP-Mosaico with the existing 480×480 GitHub Light
status screen. Solid TSX runs as JavaScript in QuickJS-NG; the PocketJS UI core,
receiver and Embassy integration remain `no_std`. MicroTS is not used.

The default firmware relays Vial. A Rynk build is also available. The display
consumes RMK events for the wireless link, USB state, active layer, six held keys,
modifiers, and the keyboard's left/right battery readings.

## Build

Install stable Rust, Bun, Git, Python 3, and a host C compiler. The tested host is
macOS arm64. From the repository root:

```sh
./tools/setup.sh --s31
./tools/build-ui.sh
cd firmware/esp32s31
cargo build --release --locked
```

For a keyboard using Rynk:

```sh
cargo build --release --locked --no-default-features --features rynk
```

Both commands produce `firmware/esp32s31/target/riscv32imafc-unknown-none-elf/release/rmk-mosaico-quickjs`.
Use the same configurator protocol on the keyboard and receiver. Pairing storage
remains at `0xff0000` in the last 64 KiB of the 16 MiB flash. Changing RMK's
commit or enabled features changes its storage schema and can require pairing again;
keeping the flash partition intact alone does not prevent that reset.

The setup script fetches pinned source into `.deps`, applies the recorded
patches, and installs an isolated Espressif RISC-V C toolchain there. Generated
JS and font/style assets are checked in, so rebuilding them is only necessary
after editing the UI.

## Host preview and checks

```sh
cargo test --release --locked
cargo run --release --locked --example embassy
cargo run --release --locked --example preview
python3 tools/preview.py
```

The preview writes `target/preview.png` using sample keyboard state. The Embassy
example runs 60 layer updates while a separate heartbeat task continues. These
host executables use `std` for the harness; the shared UI library does not.

`./tools/verify.sh` builds and lints Vial, Rynk, and the optional PPA configurations.
It saves each ELF and its static memory report under `target/artifacts`.
The checks never flash or reset hardware.

Host tests cover typed state transfer, literal key labels, sub-millisecond
press/release colors, modifier release, settling back to the idle image,
incremental versus full redraw, elapsed-time layer motion, charging-icon geometry,
JS errors, execution budgets and immutable asset buffers. The RMK patch includes
a regression test for a display subscribing after the keyboard has connected.

The Vial build has been tested on ESP-Mosaico: the display works, the keyboard
reconnects, and Vial returns protocol version 9. Rynk is build-checked. Long-duration
radio testing and stack high-water measurements remain separate from these checks.

## Flash

Use `espflash 4.6.0` for image generation and `esptool 5.4.0` for writing. The
[runner](firmware/esp32s31/run.sh) keeps the verified esptool USB-OTG path.
From `firmware/esp32s31`, enter ROM download mode with BOOT + POWER, then run:

```sh
cargo run --release --locked -- --port PORT
```

Replace `PORT` with the newly appearing download port. Add
`--no-default-features --features rynk` before `--` for Rynk. Once this firmware
runs, [`reboot.py`](firmware/esp32s31/reboot.py) can request download mode over USB.
The receiver keeps the existing USB VID/PID, product name and BLE address derivation.

## Execution and memory

Core 0 runs RMK's USB, BLE, storage and display-state collection tasks. Core 1
owns QuickJS and the CO5300 panel. A Watch carries the latest keyboard snapshot,
so a slow UI update cannot create a queue of obsolete key states. Held-key boxes
show current state; the press counter preserves typing-activity feedback for short
taps. Dongle link state is retained for late subscribers.

Animations use monotonic elapsed time. New keyboard state wakes the UI at once.
Native-only animations wait for the next 60 Hz tick; JavaScript work follows its
own deadline. Settled state waits for events. Dirty windows keep the existing X/Y
alignment and continuous-CS DMA transfer behavior.

The status app publishes its next JS wake time. Native-only animation frames
reuse the retained scene. Registered framework callbacks, timers and services
keep JS awake when required; a script without the status app's optional scheduling
globals receives ordinary frame calls. The framework skips empty input/lifecycle
work automatically, without disabling callbacks for a replacement interface.

The pinned S31 linker divides the 512 KiB HP SRAM into about 435.9 KiB of ordinary
RAM, 64 KiB reclaimable after boot, and 12.1 KiB reserved for ROM. The reclaimed
region is now used explicitly rather than leaving it empty.

| Allocation | Location | Budget |
| --- | --- | --- |
| Controller, RTOS and newlib C heap | Internal SRAM: reclaimed + ordinary | 64 + 8 = 72 KiB |
| UI executor stack | Ordinary internal SRAM | 192 KiB, including a 128 KiB JS stack limit |
| SPI DMA buffers | Ordinary internal SRAM | 2 × 30 KiB |
| QuickJS interpreter and 17 VM helpers | Ordinary internal SRAM | About 36 KiB with `-O2`; remaining engine code stays in flash |
| Core 0 stack | Remaining ordinary SRAM | See the linked ELF memory report |
| Panel wire framebuffer | PSRAM prefix | 450 KiB |
| Rust UI, QuickJS allocations and retained UI framebuffer | PSRAM GUI heap | Remaining PSRAM; JS engine heap limit 4 MiB |
| Embedded JS, styles and fonts | Flash | Checked-in bundle |

The allocator uses TLSF and two region slots: the controller needs two regions
and the GUI needs one. QuickJS allocation calls are redirected to the Rust GUI
allocator, including its usable-size callback for memory accounting. Global C and
reentrant newlib allocation calls use `esp_alloc::HEAP` in internal RAM for the
radio. The radio's formatter and QuickJS's newlib formatter have separate symbol
names. The controller's 72 KiB heap budget matches RMK/esp-hal BLE examples; it is not a
measured minimum. UART logs report free internal memory
after controller initialization; debug logging also includes render timing.

The C port supplies a monotonic `clock_gettime` and newlib reentrancy state.
QuickJS time reads use device uptime; they do not provide a wall-clock date.

A live `JsUi` stays on its owning executor and is not `Send`. JS evaluation and
native rendering are synchronous. The interrupt-poll budget stops runaway JS;
it does not make GC/rendering preemptible or establish a real-time latency bound.
A guest error stops further UI drawing while the receiver tasks continue.

## Optional on-device measurements

Build with `--features usb-diagnostics` to capture a one-shot startup benchmark.
It uses the same 80 ms logical input timeline for each build and records a hash
of every rendered frame. Probes automatically stop afterwards; normal builds
omit the instrumentation. The benchmark does not send keyboard input to the host.

```sh
uv run tools/read-diagnostics.py --profile --fixtures --output target/measurement.json
```

Build with `--features frame-benchmark` for four display-only workloads with
real panel transfers. Each workload warms up for at least one second and 32
attempts, then records 192 attempts. The result buffer is preallocated, detailed
probes stay disabled, and timing stops when the benchmark finishes. Read samples
after completion so USB diagnostic traffic does not disturb the measured run.

```sh
uv run tools/read-diagnostics.py --presents --output target/frames.json
python3 tools/summarize-fps.py target/frames.json --output target/fps.json
```

The optional `ppa-render` feature runs an Embassy renderer over PPA/DMA2D.
It sends opaque fills of at least 8192 pixels to hardware and keeps other
operations in software. It remains disabled by default: the current status
screen showed no consistent net FPS gain. Hardware blending is excluded because
its RGB565 output differs from the software rounding contract.

`frame-benchmark,ppa-render` runs the same four workloads twice in one boot:
software first, then hardware. Hardware warmup frames are checked against the
software result; the comparison stops before timing. Summarize both halves with:

```sh
uv run tools/read-diagnostics.py --presents --output target/ppa-frames.json
python3 tools/summarize-fps.py target/ppa-frames.json --scenario-offset 0
python3 tools/summarize-fps.py target/ppa-frames.json --scenario-offset 4
```

The separate `ppa-probe` feature measures fill and blend primitives. Read it with
`uv run tools/read-diagnostics.py --accelerator`. Do not combine it with
`ppa-render`. The driver follows ESP-IDF revision
`0e6b480f858c69862ffb6001d96e9eef74599a6e` and uses the pinned PAC plus esp-hal's
interrupt and cache-maintenance APIs; it does not require ESP-IDF or FreeRTOS.

`perf-no-cache` measures the new scheduler without command reuse; `perf-baseline`
also restores immediate animation polling. `POCKETJS_QJS_OPT_LEVEL=s` selects the
previous `-Os` C build for comparison. The S31 default is `-O2`. Inspect the ELF
memory report before flashing a different compiler configuration.


## Source and pins

- [`ui/Screen.tsx`](ui/Screen.tsx), [`ui/components`](ui/components), and
  [`ui/themes/github-light.json`](ui/themes/github-light.json): the existing layout and palette.
- [`ui/Status.ts`](ui/Status.ts): Solid signals and time-based UI state.
- [`src/state.rs`](src/state.rs): typed state copied directly into JS values, without JSON parsing or evaluating key labels as code.
- [`firmware/esp32s31/src/display`](firmware/esp32s31/src/display): RMK event folding and presentation.
- PocketJS: `becc0e21b678384ec31efa2b4f8d0be3aeefad5f`.
- QuickJS-NG: v0.14.0, `3c051980ab7e783dfbfb1c70c014ce5e05ecf24c`.
- RMK: `e9f242b40856b857b25f160949f8c31283f35441` plus
  [`rmk-dongle-state.patch`](patches/rmk-dongle-state.patch).
- ESP stack: `163e6b62b5b3f1d1dc2cae10b6ccc68ed9e92fde`.
- C toolchain: Espressif `esp-16.1.0_20260609`, `rv32imafc` / `ilp32f`.

The [QuickJS patch](patches/quickjs-port.patch) fixes newlib `int32_t` pointer
compatibility, preserves PocketJS's immutable package-buffer contract, and puts
the S31 interpreter and property, array and object-lifecycle helpers in SRAM. `POCKETJS_QJS_IRAM=0` builds a flash-execution
control for comparison.

The [native-execution patch](patches/pocketjs-native-execution.patch) tracks paint mutations
and replays unchanged small view/text groups. It retains at most 512 node slots
and 256 command words per group; native/streamed fonts and external image or
physics content keep the full traversal. Resource changes and layout changes
invalidate the affected results. `tools/test-native.py` checks native behavior;
the UI pixel reference uses a separate uncached traversal.

The same patch places eligible absolute leaves from their parent geometry without
running the flex solver. Percentages, margins, min/max constraints, flow layout,
native/streamed text and explicit layout regions keep the general path. Text
remeasurement retains solver results when its dimensions are unchanged. The C
operation bridge decodes up to six arguments on the stack and validates them once;
public entry points retain argument and reentrancy checks. Damage planning retains
decoded operations without searching for pixel moves that this display would repaint.
A row index selects commands intersecting each dirty region. An opaque rectangle
covering the region makes the preceding clear unnecessary. The async renderer
interface keeps resources borrowed until rendering completes; a cancelled frame
forces a complete retry. Framebuffers are aligned for DMA cache maintenance.
None of these paths depends on the status screen's labels, node IDs or palette.

The [display-runtime patch](patches/pocketjs-display-runtime.patch) preserves normal
Solid conditionals that own nodes, reduces pure text-binding overhead, and detects
pending framework work. Literal class branches compile to retained bindings;
style-table changes invalidate their resolved IDs, while dynamic classes and
custom resolvers retain their existing behavior. These changes apply to replacement JS interfaces as well
as the current status screen. The host disables unused input-tape recording;
set `globalThis.__pocketRecordInput = true` before mounting to enable it.
The C SDK link uses esp-hal's stack-protection implementation. The application
uses the lower-level `Handle`/`CGuest` lifecycle with embedded bundles, rather
than the full application-package admission API.

Receiver, panel and UI code were adapted from the RMK Mosaico worktree at
`10410241b`. GPL-3.0-or-later; dependency/font notices are in [`licenses`](licenses),
and the Primer palette notice is in [`ui/themes`](ui/themes).
