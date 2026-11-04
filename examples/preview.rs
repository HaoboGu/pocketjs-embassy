use pocketjs_embassy::{HEIGHT, JsUi, WIDTH};
use std::{fs::File, io::Write};

fn main() {
    let mut ui = JsUi::new().expect("boot Solid UI in QuickJS");
    assert!(ui.contains_text("0"));
    ui.evaluate(b"setKeyCount(7)").expect("update Solid signal");
    let pixels = ui.frame().expect("draw JS update").to_vec();
    assert!(ui.contains_text("7"));
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/preview.ppm".into());
    let mut output = File::create(&path).unwrap();
    write!(output, "P6\n{WIDTH} {HEIGHT}\n255\n").unwrap();
    for pixel in pixels {
        output
            .write_all(&[
                (((pixel >> 11) & 31) * 255 / 31) as u8,
                (((pixel >> 5) & 63) * 255 / 63) as u8,
                ((pixel & 31) * 255 / 31) as u8,
            ])
            .unwrap();
    }
    println!("PASS: Solid signal 0 -> 7, rendered {WIDTH}x{HEIGHT}: {path}");
}
