use pocketjs_embassy::{HEIGHT, JsUi, WIDTH, state::Model};
use std::{fs::File, io::Write};
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let option = |name| {
        args.iter()
            .position(|arg| arg == name)
            .and_then(|i| args.get(i + 1))
    };
    let mut ui = if let Some(path) = option("--bundle") {
        let path = std::path::Path::new(path);
        JsUi::from_bundle(
            &std::fs::read(path.join("app.js")).unwrap(),
            std::fs::read(path.join("app.pak")).unwrap(),
        )
        .unwrap()
    } else {
        JsUi::new().unwrap()
    };
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
    if option("--scene").is_some_and(|scene| scene == "idle") {
        for key in &mut state.key_labels {
            key.clear();
        }
        state.modifiers = 0;
    }
    if option("--scene").is_some_and(|scene| scene == "charging") {
        state.charging_left = true;
    }
    ui.frame(state.clone(), 0, false).unwrap();
    ui.frame(state, 700_000, false).unwrap();
    assert!(ui.contains_text("Connected") && ui.contains_text("02"));
    let path = option("--output")
        .cloned()
        .or_else(|| args.first().filter(|s| !s.starts_with("--")).cloned())
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
