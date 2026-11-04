#![no_std]
#![no_main]

extern crate alloc;
mod board;

use alloc::format;
use embassy_executor::Spawner;
use embassy_time::{Duration, Instant, Ticker, Timer};
use embedded_graphics::{
    prelude::{Point, Size},
    primitives::Rectangle,
};
use esp_backtrace as _;
use esp_hal::{clock::CpuClock, system::Stack, timer::timg::TimerGroup};
use pocketjs_embassy::{HEIGHT, JsUi, WIDTH};
use static_cell::{ConstStaticCell, StaticCell};

esp_bootloader_esp_idf::esp_app_desc!();

#[global_allocator]
static GUI_HEAP: esp_alloc::EspHeap = esp_alloc::EspHeap::empty();

#[unsafe(no_mangle)]
extern "C" fn pocket_uptime_us() -> u64 {
    Instant::now().as_micros()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn pocket_uart_write(bytes: *const u8, length: usize) {
    // Newlib keeps this output buffer readable until the synchronous write returns.
    esp_println::Printer::write_bytes(unsafe { core::slice::from_raw_parts(bytes, length) });
}

#[embassy_executor::task]
async fn ui_task(peripherals: board::DisplayPeripherals) {
    let Some(mut panel) = board::init(peripherals).await else {
        return;
    };
    let mut ui = JsUi::new().expect("initialize Solid/QuickJS");
    let mut ticker = Ticker::every(Duration::from_micros(16_667));
    let area = Rectangle::new(Point::zero(), Size::new(WIDTH as u32, HEIGHT as u32));
    let mut previous = u64::MAX;
    loop {
        let count = Instant::now().as_secs() % 10;
        if count != previous {
            ui.evaluate(format!("setKeyCount({count})").as_bytes())
                .unwrap();
            let started = Instant::now();
            let pixels = ui.frame().unwrap();
            panel.present_rgb565(pixels, area).await.unwrap();
            log::info!(
                "QuickJS count={} render+present={}us",
                count,
                started.elapsed().as_micros()
            );
            previous = count;
        }
        ticker.next().await;
    }
}

#[esp_rtos::main]
async fn main(_spawner: Spawner) {
    esp_println::logger::init_logger_from_env();
    let p = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    let psram = esp_hal::psram::Psram::new(p.PSRAM, Default::default());
    let (start, size) = psram.raw_parts();
    let framebuffer = WIDTH * HEIGHT * 2;
    assert!(size > framebuffer);
    // The panel owns the prefix; Rust and QuickJS allocate from the remaining PSRAM.
    unsafe {
        GUI_HEAP.add_region(esp_alloc::HeapRegion::new(
            start.add(framebuffer),
            size - framebuffer,
            esp_alloc::MemoryCapability::External.into(),
        ));
    }
    esp_alloc::heap_allocator!(size: 32 * 1024);
    esp_rtos::start(TimerGroup::new(p.TIMG0).timer0);
    let display = board::DisplayPeripherals {
        spi2: p.SPI2,
        dma: p.DMA_AXI_CH0,
        psram,
        power: p.GPIO60,
        gpio42: p.GPIO42,
        gpio44: p.GPIO44,
        data0: p.GPIO36,
        data1: p.GPIO51,
        data2: p.GPIO35,
        data3: p.GPIO9,
        cs: p.GPIO50,
    };
    static STACK: ConstStaticCell<Stack<{ 192 * 1024 }>> = ConstStaticCell::new(Stack::new());
    static EXECUTOR: StaticCell<esp_rtos::embassy::Executor> = StaticCell::new();
    esp_rtos::start_second_core(p.CPU_CTRL, STACK.take(), move || {
        EXECUTOR
            .init(esp_rtos::embassy::Executor::new())
            .run(|spawner| {
                spawner.spawn(ui_task(display).unwrap());
            });
    });
    loop {
        Timer::after_secs(1).await;
        log::info!("Embassy core 0 heartbeat");
    }
}
