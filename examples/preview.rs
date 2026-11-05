use pocketjs_embassy::{HEIGHT, JsUi, WIDTH, state::Model};
use std::{fs::File, io::Write};
fn main() {
    let mut ui = JsUi::new().unwrap();
    let mut state = Model {
        link: "Connected",
        usb: "Ready",
        layer: 2,
        battery_left: 85,
        battery_right: 72,
        modifiers: 0x02,
        ..Model::default()
    };
    for (slot, label) in state.key_labels.iter_mut().zip(["A", "S", "D", "Enter"]) {
        slot.push_str(label).unwrap();
    }
    ui.frame(state.clone(), 0, false).unwrap();
    ui.frame(state, 700_000, false).unwrap();
    assert!(ui.contains_text("Connected") && ui.contains_text("02"));
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/preview.ppm".into());
    let mut output = File::create(&path).unwrap();
    write!(output, "P6\n{WIDTH} {HEIGHT}\n255\n").unwrap();
    for &pixel in ui.pixels() {
        output
            .write_all(&[
                (((pixel >> 11) & 31) * 255 / 31) as u8,
                (((pixel >> 5) & 63) * 255 / 63) as u8,
                ((pixel & 31) * 255 / 31) as u8,
            ])
            .unwrap();
    }
    println!("PASS: RMK Mosaico state rendered through Solid/QuickJS: {path}");
}
