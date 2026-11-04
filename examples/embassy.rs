use embassy_executor::{Executor, Spawner};
use embassy_time::{Duration, Ticker, Timer};
use pocketjs_embassy::JsUi;
use static_cell::StaticCell;
use std::sync::atomic::{AtomicU32, Ordering};

static HEARTBEATS: AtomicU32 = AtomicU32::new(0);

#[embassy_executor::task]
async fn heartbeat() {
    loop {
        HEARTBEATS.fetch_add(1, Ordering::Relaxed);
        Timer::after_millis(1).await;
    }
}

#[embassy_executor::task]
async fn ui_probe() {
    let mut ui = JsUi::new().unwrap();
    let mut ticker = Ticker::every(Duration::from_micros(16_667));
    for value in 0..60 {
        ui.evaluate(format!("setKeyCount({value})").as_bytes())
            .unwrap();
        ui.frame().unwrap();
        assert!(ui.contains_text(&value.to_string()));
        ticker.next().await;
    }
    let beats = HEARTBEATS.load(Ordering::Relaxed);
    assert!(beats >= 60, "heartbeat task did not make progress: {beats}");
    drop(ui);
    println!("PASS: 60 Solid/QuickJS updates under Embassy; heartbeat ran {beats} times");
    std::process::exit(0);
}

fn main() {
    static EXECUTOR: StaticCell<Executor> = StaticCell::new();
    EXECUTOR.init(Executor::new()).run(|spawner: Spawner| {
        spawner.spawn(heartbeat().unwrap());
        spawner.spawn(ui_probe().unwrap());
    });
}
