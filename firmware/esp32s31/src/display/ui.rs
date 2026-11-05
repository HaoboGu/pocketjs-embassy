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
        Ok(Self {
            scene: JsUi::new()?,
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
        let damage = match self.scene.frame(model, started.as_micros(), self.retry) {
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
        if started.as_micros() - self.last_log_us >= 5_000_000 {
            self.last_log_us = started.as_micros();
            log::info!(
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
