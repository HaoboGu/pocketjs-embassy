use serde::Serialize;

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct Model {
    pub link: &'static str,
    pub usb: &'static str,
    pub layer: u8,
    pub key_labels: [heapless::String<8>; 6],
    pub modifiers: u8,
    pub presses: u8,
    pub battery_left: i32,
    pub battery_right: i32,
    pub charging_left: bool,
    pub charging_right: bool,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            link: "Searching",
            usb: "Offline",
            layer: 0,
            key_labels: [const { heapless::String::new() }; 6],
            modifiers: 0,
            presses: 0,
            battery_left: -1,
            battery_right: -1,
            charging_left: false,
            charging_right: false,
        }
    }
}
