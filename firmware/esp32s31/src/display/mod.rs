//! Collect receiver state on Core 0; render the latest snapshot on Core 1.

mod ui;

use embassy_futures::select::{Either, select};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, watch::Watch};
use embassy_time::Instant;
use rmk::core_traits::Runnable;
use rmk::event::{
    ActionEvent, BatteryStatusEvent, ConnectionStatusChangeEvent, DongleState, DongleStateEvent,
    EventSubscriber, LayerChangeEvent, ModifierEvent, PeripheralBatteryEvent,
};
use rmk::futures::FutureExt;
use rmk::macros::{processor, runnable_generated};
use rmk::processor::Processor;
use rmk::types::action::Action;
use rmk::types::battery::BatteryStatus;
use rmk::types::connection::UsbState;
use rmk::types::keycode::KeyCode;
use ui::Ui;

use crate::board::Panel;

/// Keyboard state consumed by the view; unknown battery readings remain `None`.
#[derive(Clone, Copy, Default)]
pub struct Snapshot {
    pub link: DongleState,
    /// Left and right battery state, in that order.
    pub batt: [Option<BatteryStatus>; 2],
    pub usb: UsbState,
    /// Raw modifier bits allow equality comparisons in the view.
    pub mods: u8,
    /// Up to six displayed HID usages; zero marks an empty slot.
    pub keys: [u8; 6],
    /// Wrapping press tally, including modifiers, used to detect typing activity.
    pub presses: u8,
    pub layer: u8,
    pub revision: u32,
    pub changed_at_us: u64,
}

static LATEST: Watch<CriticalSectionRawMutex, Snapshot, 1> = Watch::new();

/// Keep event consumption independent of synchronous JS and drawing.
#[processor(subscribe = [
    ConnectionStatusChangeEvent,
    ModifierEvent,
    ActionEvent,
    DongleStateEvent,
    BatteryStatusEvent,
    PeripheralBatteryEvent,
    LayerChangeEvent,
])]
#[runnable_generated]
#[derive(Default)]
pub struct StateCollector {
    state: Snapshot,
}

impl StateCollector {
    async fn on_connection_status_change_event(&mut self, event: ConnectionStatusChangeEvent) {
        self.state.usb = event.0.usb;
    }

    async fn on_modifier_event(&mut self, event: ModifierEvent) {
        self.state.mods = event.modifier.into_bits();
    }

    /// Update the displayed key slots and count press events for typing speed.
    async fn on_action_event(&mut self, event: ActionEvent) {
        let pressed = event.keyboard_event.pressed;
        if pressed {
            self.state.presses = self.state.presses.wrapping_add(1);
        }
        let code = match event.action {
            Action::Key(KeyCode::Hid(key)) if !key.is_modifier() => key as u8,
            Action::KeyWithModifier(key, _) | Action::OneShotKey(key) if !key.is_modifier() => {
                key as u8
            }
            _ => return,
        };
        let keys = &mut self.state.keys;
        if !pressed {
            keys.iter_mut().filter(|k| **k == code).for_each(|k| *k = 0);
        } else if !keys.contains(&code)
            && let Some(slot) = keys.iter_mut().find(|k| **k == 0)
        {
            *slot = code;
        }
    }

    async fn on_dongle_state_event(&mut self, event: DongleStateEvent) {
        self.state.link = event.0;
    }

    async fn on_battery_status_event(&mut self, event: BatteryStatusEvent) {
        self.state.batt[0] = Some(event.0);
    }

    /// Display slot 1 belongs to the right half, peripheral 0.
    async fn on_peripheral_battery_event(&mut self, event: PeripheralBatteryEvent) {
        if event.id == 0 {
            self.state.batt[1] = Some(event.state.0);
        }
    }

    async fn on_layer_change_event(&mut self, event: LayerChangeEvent) {
        self.state.layer = event.0;
    }
}

impl Runnable for StateCollector {
    async fn run(&mut self) -> ! {
        let mut events = Self::subscriber();
        let sender = LATEST.sender();
        self.state.usb = rmk::state::current_connection_status().usb;
        sender.send(self.state);
        loop {
            self.process(events.next_event().await).await;
            while let Some(event) = events.next_event().now_or_never() {
                self.process(event).await;
            }
            self.state.revision = self.state.revision.wrapping_add(1);
            self.state.changed_at_us = Instant::now().as_micros();
            #[cfg(feature = "usb-diagnostics")]
            crate::diagnostics::INPUT_REVISION
                .store(self.state.revision, core::sync::atomic::Ordering::Relaxed);
            sender.send(self.state);
        }
    }
}

pub async fn run(mut panel: Panel) -> Result<(), pocketjs_embassy::Error> {
    let mut receiver = LATEST.receiver().unwrap();
    #[cfg(feature = "usb-diagnostics")]
    {
        let deadline = Instant::now() + embassy_time::Duration::from_secs(20);
        let mut snapshot = receiver.get().await;
        while !(matches!(snapshot.link, DongleState::Connected)
            && snapshot.usb == UsbState::Configured)
        {
            match select(receiver.changed(), embassy_time::Timer::at(deadline)).await {
                Either::First(next) => snapshot = next,
                Either::Second(_) => break,
            }
        }
        embassy_time::Timer::after_millis(1500).await;
        log::info!(
            "JS benchmark link={:?} usb={:?}",
            snapshot.link,
            snapshot.usb
        );
    }
    #[cfg(feature = "ppa-probe")]
    crate::acceleration::probe().await;
    let mut ui = Ui::new()?;
    #[cfg(feature = "frame-benchmark")]
    ui.benchmark(&mut panel).await?;
    let mut state = receiver.get().await;
    loop {
        ui.render(&state, &mut panel).await;
        if let Either::Second(next) = select(ui.wait(), receiver.changed()).await {
            state = next;
        }
        if let Some(next) = receiver.try_changed() {
            state = next;
        }
    }
}
