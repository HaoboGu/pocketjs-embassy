use pocketjs_embassy::{JsUi, state::Model};
use std::{fs, path::PathBuf};
fn main() {
    let path = PathBuf::from(std::env::args().nth(1).expect("bundle directory"));
    let javascript = fs::read(path.join("app.js")).unwrap();
    let assets = fs::read(path.join("app.pak")).unwrap();
    let mut ui = JsUi::new().unwrap();
    let mut model = Model {
        link: "Connected",
        usb: "Ready",
        battery_left: 85,
        battery_right: 72,
        ..Model::default()
    };
    ui.frame(model.clone(), 0, true).unwrap();
    ui.reload(&javascript, assets.clone(), 1_000_000).unwrap();
    for index in 0..180u64 {
        model.layer = (index / 24 % 4) as u8;
        model.modifiers = if index % 5 == 0 { 0x55 } else { 0 };
        model.key_labels[0].clear();
        model.key_labels[1].clear();
        if index % 3 != 0 {
            model.key_labels[0].push_str("Enter").unwrap();
        }
        if index % 7 < 3 {
            model.key_labels[1].push_str("A").unwrap();
        }
        model.charging_left = index % 40 < 20;
        model.battery_right = (index / 2 % 101) as i32;
        let now = 1_000_000 + index * 16_667;
        if index % 60 == 0 {
            ui.reload(&javascript, assets.clone(), now).unwrap();
        }
        ui.frame(model.clone(), now, false).unwrap();
        assert_eq!(ui.pixels(), ui.reference_pixels(), "frame {index}");
    }
    println!(
        "PASS: 180 changing frames and repeated reloads matched full rendering: {}",
        path.display()
    );
}
