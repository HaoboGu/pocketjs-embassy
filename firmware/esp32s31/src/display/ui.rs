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
    #[cfg(feature = "ppa-render")]
    accelerator: crate::acceleration::Renderer,
}
impl Ui {
    pub fn new() -> Result<Self, pocketjs_embassy::Error> {
        let scene = JsUi::new()?;
        #[cfg(all(feature = "usb-diagnostics", not(feature = "frame-benchmark")))]
        let scene = {
            crate::diagnostics::prepare_fixtures();
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
            #[cfg(feature = "ppa-render")]
            accelerator: crate::acceleration::Renderer::new(),
        })
    }
    async fn draw_frame(
        &mut self,
        model: Model,
        now_us: u64,
        force: bool,
    ) -> Result<Option<pocket_render::damage::DamagePlan<8>>, pocketjs_embassy::Error> {
        #[cfg(feature = "ppa-render")]
        {
            self.scene
                .frame_with_renderer(model, now_us, force, &mut self.accelerator)
                .await
        }
        #[cfg(not(feature = "ppa-render"))]
        {
            self.scene.frame(model, now_us, force)
        }
    }

    #[cfg(feature = "frame-benchmark")]
    pub async fn benchmark(&mut self, panel: &mut Panel) -> Result<(), pocketjs_embassy::Error> {
        crate::diagnostics::MEASURING.store(false, core::sync::atomic::Ordering::Relaxed);
        self.scene.evaluate(b"delete globalThis.__pocketProfile")?;
        crate::diagnostics::COARSE_MEASURING.store(true, core::sync::atomic::Ordering::Relaxed);
        crate::diagnostics::prepare_presents();
        for scenario in 0..if cfg!(feature = "ppa-render") {
            8u32
        } else {
            4u32
        } {
            let workload = scenario % 4;
            #[cfg(feature = "ppa-render")]
            {
                self.accelerator.enabled = scenario >= 4;
            }
            let mut model = Model {
                link: "Connected",
                usb: "Ready",
                battery_left: 85,
                battery_right: 72,
                charging_left: workload == 1,
                ..Model::default()
            };
            let mut previous_complete = 0;
            let mut scenario_start = Instant::now().as_micros();
            let mut last_input = 0;
            let mut pressed = false;
            let warmup_start = Instant::now();
            let mut index = 0;
            let mut samples = 0;
            // Cover entry and typing-release effects even when renders are very short.
            loop {
                let now = Instant::now().as_micros();
                let measuring = index >= 32 && warmup_start.elapsed().as_millis() >= 1000;
                if measuring && samples == 0 {
                    scenario_start = now;
                    previous_complete = now;
                }
                let change = workload == 0
                    || (workload == 2 && now - last_input >= 80_000)
                    || (workload == 3 && now - last_input >= 240_000);
                if change {
                    last_input = now;
                    pressed = !pressed;
                    model.key_labels[0].clear();
                    if pressed {
                        model.key_labels[0].push_str("A").unwrap();
                        model.presses = model.presses.wrapping_add(1);
                    }
                    if workload == 3 {
                        model.modifiers = if pressed { 5 } else { 0 };
                        model.layer = (model.layer + 1) % 3;
                    }
                }
                let started = Instant::now();
                #[cfg(feature = "ppa-render")]
                {
                    self.accelerator.validate = !measuring && scenario >= 4;
                }
                let damage = self
                    .draw_frame(model.clone(), started.as_micros(), index == 0)
                    .await?;
                let render_us = started.elapsed().as_micros();
                for region in damage.iter().flat_map(|plan| plan.regions()) {
                    let area = Rectangle::new(
                        Point::new(region.x0, region.y0),
                        Size::new(
                            (region.x1 - region.x0) as u32,
                            (region.y1 - region.y0) as u32,
                        ),
                    );
                    if let Err(error) = panel.present_rgb565(self.scene.pixels(), area).await {
                        log::error!("Frame benchmark transfer failed: {error:?}");
                        return Ok(());
                    }
                }
                let complete = Instant::now().as_micros();
                if measuring {
                    crate::diagnostics::record_present([
                        scenario,
                        render_us as u32,
                        (complete - started.as_micros() - render_us) as u32,
                        (complete - started.as_micros()) as u32,
                        (complete - previous_complete) as u32,
                        damage.map_or(0, |p| p.area()) as u32,
                        u32::from(change),
                        (complete - scenario_start) as u32,
                    ]);
                    previous_complete = complete;
                    samples += 1;
                    if samples == 192 {
                        break;
                    }
                }
                index += 1;
                if workload != 0 {
                    let input = match workload {
                        2 => last_input + 80_000,
                        3 => last_input + 240_000,
                        _ => u64::MAX,
                    };
                    let deadline = self.scene.next_frame_us().unwrap_or(u64::MAX).min(input);
                    Timer::at(Instant::from_micros(deadline)).await;
                }
                embassy_futures::yield_now().await;
            }
        }
        #[cfg(feature = "ppa-render")]
        {
            self.accelerator.validate = false;
            log::info!(
                "PPA renderer calls={} pixels={} failures={} mismatched_frames={} verified={}",
                self.accelerator.calls,
                self.accelerator.pixels,
                self.accelerator.failures,
                self.accelerator.mismatched_frames,
                self.accelerator.verified_frames
            );
        }
        log::info!("Draw cache info {:?}", self.scene.cache_info());
        crate::diagnostics::COARSE_MEASURING.store(false, core::sync::atomic::Ordering::Relaxed);
        log::info!("Full-frame benchmark complete; detailed probes disabled");
        Ok(())
    }
    pub async fn wait(&self) {
        if self.failed {
            core::future::pending::<()>().await;
        }
        if let Some(deadline) = self.defer_until {
            Timer::at(deadline).await;
        } else if !self.rendered || self.retry {
            embassy_futures::yield_now().await;
        } else if let Some(deadline) = self.scene.next_frame_us() {
            Timer::at(Instant::from_micros(deadline)).await;
        } else {
            core::future::pending::<()>().await;
        }
    }
    pub fn reload(
        &mut self,
        bundle: pocketjs_embassy::reload::Bundle,
    ) -> Result<(), pocketjs_embassy::Error> {
        self.scene.reload(
            &bundle.javascript,
            bundle.assets,
            Instant::now().as_micros(),
        )?;
        self.rendered = false;
        self.retry = true;
        self.failed = false;
        self.defer_until = None;
        Ok(())
    }
    pub async fn render(&mut self, state: &Snapshot, panel: &mut Panel) -> bool {
        if self.failed {
            return false;
        }
        let now = Instant::now();
        if !self.rendered && state.usb != UsbState::Configured && now.as_millis() < 3_000 {
            self.defer_until = Some(Instant::from_millis(3_000));
            return false;
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
        let damage = match self
            .draw_frame(model, started.as_micros(), self.retry || !self.rendered)
            .await
        {
            Ok(damage) => damage,
            Err(error) => {
                log::error!("QuickJS display stopped: {error:?}");
                self.failed = true;
                return false;
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
        !self.retry
    }
}
