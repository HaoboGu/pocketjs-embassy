//! Fold relayed keyboard events into a snapshot and repaint it between event bursts.

mod ui;

use embassy_futures::select::{Either, select};
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
}

/// Own the panel and yield during its transfers so transports can run.
///
/// Subscriptions are polled in order, so connection and modifier updates apply before
/// key actions and status events.
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
pub struct DongleDisplay {
    panel: Panel,
    ui: Ui,
    state: Snapshot,
}

type Event = <DongleDisplay as Processor>::Event;

impl DongleDisplay {
    pub fn new(panel: Panel) -> Result<Self, pocketjs_embassy::Error> {
        Ok(Self {
            panel,
            ui: Ui::new()?,
            state: Snapshot::default(),
        })
    }

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
        let Some(code) = hid_code(&event) else {
            return;
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

/// Extract a displayable HID usage; modifiers use their separate event.
fn hid_code(event: &ActionEvent) -> Option<u8> {
    match event.action {
        Action::Key(KeyCode::Hid(key)) if !key.is_modifier() => Some(key as u8),
        Action::KeyWithModifier(key, _) | Action::OneShotKey(key) if !key.is_modifier() => {
            Some(key as u8)
        }
        _ => None,
    }
}

/// Whether `event` would reverse a change made since `start` before it is painted.
fn undoes(start: &Snapshot, now: &Snapshot, event: &Event) -> bool {
    match event {
        Event::Action(action) => hid_code(action).is_some_and(|code| {
            let pressed = action.keyboard_event.pressed;
            start.keys.contains(&code) == pressed && now.keys.contains(&code) != pressed
        }),
        Event::Modifier(event) => now.mods & !start.mods & !event.modifier.into_bits() != 0,
        _ => false,
    }
}

impl Runnable for DongleDisplay {
    async fn run(&mut self) -> ! {
        self.run_with_events(&mut Self::subscriber()).await
    }
}

impl DongleDisplay {
    pub async fn run_with_events(&mut self, events: &mut impl EventSubscriber<Event = Event>) -> ! {
        self.state.usb = rmk::state::current_connection_status().usb;
        // A reversing event starts the next paint after this state has been displayed.
        let mut held = None;
        loop {
            let start = self.state;
            if held.is_none()
                && let Either::Second(event) = select(self.ui.wait(), events.next_event()).await
            {
                held = Some(event);
            }
            // Coalesce queued updates until the first visible transition would disappear.
            while let Some(event) = held.take().or_else(|| events.next_event().now_or_never()) {
                if undoes(&start, &self.state, &event) {
                    held = Some(event);
                    break;
                }
                self.process(event).await;
            }
            self.ui.render(&self.state, &mut self.panel).await;
        }
    }
}
