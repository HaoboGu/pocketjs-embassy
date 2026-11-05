use super::Snapshot;
use crate::board::Panel;
use core::fmt::Write;
use embassy_time::{Instant, Timer};
use embedded_graphics::{
    prelude::{Point, Size},
    primitives::Rectangle,
};
use pocketjs_embassy::{JsUi, state::Model};
use rmk::event::DongleState;
use rmk::types::{
    battery::{BatteryStatus, ChargeState},
    connection::UsbState,
    keycode::{HidKeyCode, to_ascii},
};

pub struct Ui {
    scene: JsUi,
    rendered: bool,
    defer_until: Option<Instant>,
    last_log_us: u64,
    retry: bool,
    failed: bool,
}
impl Ui {
    pub fn new() -> Result<Self, pocketjs_embassy::Error> {
        let scene = JsUi::new()?;
        #[cfg(feature = "usb-diagnostics")]
        let scene = {
            let mut scene = scene;
            for (name, source) in [
                (
                    "arithmetic",
                    "(()=>{let x=0;for(let i=0;i<10000;i++)x+=i;globalThis.__probe=x})()",
                ),
                (
                    "properties",
                    "(()=>{let x=0,o={a:1,b:2};for(let i=0;i<10000;i++)x+=o.a+o.b;globalThis.__probe=x})()",
                ),
                (
                    "calls",
                    "(()=>{let x=0;const f=v=>v+1;for(let i=0;i<1000;i++)x=f(x);globalThis.__probe=x})()",
                ),
                (
                    "native_queries",
                    "(()=>{let x=0;for(let i=0;i<100;i++)x+=ui.measureText('M',2);globalThis.__probe=x})()",
                ),
            ] {
                use core::sync::atomic::Ordering;
                crate::diagnostics::ALLOC_CYCLES.store(0, Ordering::Relaxed);
                crate::diagnostics::ALLOC_CALLS.store(0, Ordering::Relaxed);
                let started = Instant::now();
                scene.evaluate(source.as_bytes())?;
                log::info!(
                    "Runtime probe {} elapsed={}us allocation={}us alloc_calls={}",
                    name,
                    started.elapsed().as_micros(),
                    crate::diagnostics::ALLOC_CYCLES.load(Ordering::Relaxed)
                        / (esp_hal::clock::cpu_clock().as_hz() / 1_000_000),
                    crate::diagnostics::ALLOC_CALLS.load(Ordering::Relaxed)
                );
            }
            scene.evaluate(b"delete globalThis.__probe")?;
            let mut model = Model {
                link: "Connected",
                usb: "Ready",
                battery_left: 85,
                battery_right: 72,
                ..Model::default()
            };
            let mut logical_us =
                (Instant::now().as_micros() / 1_000_000).saturating_sub(1) * 1_000_000;
            scene.frame(model.clone(), logical_us, true)?;
            for _ in 0..13 {
                model.key_labels[0].push_str("A").unwrap();
                model.presses = model.presses.wrapping_add(1);
                logical_us += 80_000;
                let started = Instant::now();
                let first_cycle: u32;
                unsafe {
                    core::arch::asm!("csrr {0}, mcycle", out(reg) first_cycle);
                }
                scene.frame(model.clone(), logical_us, false)?;
                let last_cycle: u32;
                unsafe {
                    core::arch::asm!("csrr {0}, mcycle", out(reg) last_cycle);
                }
                let elapsed = started.elapsed().as_micros();
                let hash = scene.pixels().iter().fold(0x811c9dc5u32, |hash, pixel| {
                    (hash ^ u32::from(*pixel)).wrapping_mul(0x01000193)
                });
                crate::diagnostics::report_detail(
                    "key_down",
                    elapsed,
                    hash,
                    last_cycle.wrapping_sub(first_cycle),
                );
                model.key_labels[0].clear();
                logical_us += 80_000;
                let started = Instant::now();
                let first_cycle: u32;
                unsafe {
                    core::arch::asm!("csrr {0}, mcycle", out(reg) first_cycle);
                }
                scene.frame(model.clone(), logical_us, false)?;
                let last_cycle: u32;
                unsafe {
                    core::arch::asm!("csrr {0}, mcycle", out(reg) last_cycle);
                }
                let elapsed = started.elapsed().as_micros();
                let hash = scene.pixels().iter().fold(0x811c9dc5u32, |hash, pixel| {
                    (hash ^ u32::from(*pixel)).wrapping_mul(0x01000193)
                });
                crate::diagnostics::report_detail(
                    "key_up",
                    elapsed,
                    hash,
                    last_cycle.wrapping_sub(first_cycle),
                );
            }
            while Instant::now().as_micros() < logical_us {
                core::hint::spin_loop();
            }
            scene.evaluate(b"delete globalThis.__pocketProfile")?;
            crate::diagnostics::MEASURING.store(false, core::sync::atomic::Ordering::Relaxed);
            scene
        };
        #[cfg(feature = "usb-diagnostics")]
        crate::diagnostics::STAGE.store(3, core::sync::atomic::Ordering::Relaxed);
        Ok(Self {
            scene,
            rendered: false,
            defer_until: None,
            last_log_us: 0,
            retry: false,
            failed: false,
        })
    }
    pub async fn wait(&self) {
        if self.failed {
            core::future::pending::<()>().await;
        }
        if let Some(deadline) = self.defer_until {
            Timer::at(deadline).await;
        } else if !self.rendered || self.retry || self.scene.animating() {
            embassy_futures::yield_now().await;
        } else {
            core::future::pending::<()>().await;
        }
    }
    pub async fn render(&mut self, state: &Snapshot, panel: &mut Panel) {
        if self.failed {
            return;
        }
        let now = Instant::now();
        if !self.rendered && state.usb != UsbState::Configured && now.as_millis() < 3_000 {
            self.defer_until = Some(Instant::from_millis(3_000));
            return;
        }
        self.defer_until = None;
        let batteries = state.batt.map(|battery| match battery {
            Some(BatteryStatus::Available {
                level,
                charge_state,
            }) => (
                level.map_or(-1, i32::from),
                charge_state == ChargeState::Charging,
            ),
            _ => (-1, false),
        });
        let key_labels = state.keys.map(|code| {
            let mut label = heapless::String::<8>::new();
            let Some(key) = HidKeyCode::from_repr(code).filter(|key| *key != HidKeyCode::No) else {
                return label;
            };
            if (HidKeyCode::A as u8..=HidKeyCode::Kc0 as u8).contains(&code)
                || (HidKeyCode::Minus as u8..=HidKeyCode::Slash as u8).contains(&code)
                || key == HidKeyCode::NonusBackslash
            {
                let symbol = match key {
                    HidKeyCode::NonusHash => b'#',
                    HidKeyCode::NonusBackslash => b'\\',
                    _ => to_ascii(key, false).to_ascii_uppercase(),
                };
                label.push(symbol as char).unwrap();
            } else {
                let short = match key {
                    HidKeyCode::Space => "Space",
                    HidKeyCode::Enter => "Enter",
                    HidKeyCode::Escape => "Esc",
                    HidKeyCode::Backspace => "Bksp",
                    HidKeyCode::CapsLock => "Caps",
                    HidKeyCode::PageUp => "PgUp",
                    HidKeyCode::PageDown => "PgDn",
                    HidKeyCode::Delete => "Del",
                    _ => "",
                };
                if short.is_empty() {
                    if write!(label, "{key:?}").is_err() || label.len() > 6 {
                        label.clear();
                        write!(label, "0x{code:02X}").unwrap();
                    }
                } else {
                    label.push_str(short).unwrap();
                }
            }
            label
        });
        let model = Model {
            link: match state.link {
                DongleState::Searching => "Searching",
                DongleState::Pairing => "Pairing",
                DongleState::Connected => "Connected",
            },
            usb: match state.usb {
                UsbState::Disabled => "Offline",
                UsbState::Enabled => "Starting",
                UsbState::Configured => "Ready",
                UsbState::Suspended => "Suspended",
            },
            layer: state.layer,
            key_labels,
            modifiers: state.mods,
            presses: state.presses,
            battery_left: batteries[0].0,
            battery_right: batteries[1].0,
            charging_left: batteries[0].1,
            charging_right: batteries[1].1,
        };

        let started = Instant::now();
        let damage =
            match self
                .scene
                .frame(model, started.as_micros(), self.retry || !self.rendered)
            {
                Ok(damage) => damage,
                Err(error) => {
                    log::error!("QuickJS display stopped: {error:?}");
                    self.failed = true;
                    return;
                }
            };
        let render_us = started.elapsed().as_micros();
        self.retry = false;
        self.rendered = true;
        #[cfg(feature = "usb-diagnostics")]
        crate::diagnostics::STAGE.store(4, core::sync::atomic::Ordering::Relaxed);
        for region in damage.iter().flat_map(|plan| plan.regions()) {
            let area = Rectangle::new(
                Point::new(region.x0, region.y0),
                Size::new(
                    (region.x1 - region.x0) as u32,
                    (region.y1 - region.y0) as u32,
                ),
            );
            if let Err(error) = panel.present_rgb565(self.scene.pixels(), area).await {
                log::error!("QuickJS display transfer failed: {error:?}");
                self.retry = true;
                break;
            }
        }
        #[cfg(feature = "usb-diagnostics")]
        if !self.retry {
            crate::diagnostics::STAGE.store(5, core::sync::atomic::Ordering::Relaxed);
            if damage.is_some() {
                crate::diagnostics::FRAMES.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            }
        }
        #[cfg(feature = "usb-diagnostics")]
        if !self.retry
            && crate::diagnostics::APPLIED_REVISION.load(core::sync::atomic::Ordering::Relaxed)
                != state.revision
        {
            crate::diagnostics::SNAPSHOT_AGE_US.store(
                started.as_micros().saturating_sub(state.changed_at_us) as u32,
                core::sync::atomic::Ordering::Relaxed,
            );
            crate::diagnostics::APPLY_LATENCY_US.store(
                Instant::now()
                    .as_micros()
                    .saturating_sub(state.changed_at_us) as u32,
                core::sync::atomic::Ordering::Relaxed,
            );
            crate::diagnostics::APPLIED_REVISION
                .store(state.revision, core::sync::atomic::Ordering::Relaxed);
        }
        if started.as_micros() - self.last_log_us >= 5_000_000 {
            self.last_log_us = started.as_micros();
            log::debug!(
                "QuickJS render={}us frame={}us dirty={}px gui_free={} internal_free={}",
                render_us,
                started.elapsed().as_micros(),
                damage.map_or(0, |plan| plan.area()),
                crate::allocation::GUI_HEAP.free(),
                esp_alloc::HEAP.free()
            );
        }
    }
}
