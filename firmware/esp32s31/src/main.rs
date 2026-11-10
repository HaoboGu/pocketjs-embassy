#![no_std]
#![no_main]

#[cfg(any(feature = "ppa-probe", feature = "ppa-render"))]
mod acceleration;
#[cfg(all(feature = "ppa-probe", feature = "ppa-render"))]
compile_error!("run the PPA primitive probe separately from the renderer");
mod allocation;
mod board;
#[cfg(feature = "usb-diagnostics")]
mod diagnostics;
mod display;
mod reload;
mod runtime_support;
mod usb_boot;

#[cfg(all(feature = "vial", feature = "rynk"))]
compile_error!("select either vial or rynk");
#[cfg(not(any(feature = "vial", feature = "rynk")))]
compile_error!("select either vial or rynk");

use bt_hci::controller::ExternalController;
use embassy_executor::Spawner;
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::system::Stack;
use esp_hal::timer::timg::TimerGroup;
use esp_hal::usb::otg::Usb;
use esp_hal::usb::otg::embassy_usb_device::{Config, Driver};
use esp_radio::ble::controller::BleConnector;
use esp_storage::FlashStorage;
use rmk::config::{DeviceConfig, StorageConfig};
use rmk::dongle::{Dongle, DongleRouter};
use rmk::run_all;
use rmk::storage::{async_flash_wrapper, new_storage_without_keymap};
use rmk::usb::UsbTransport;
use static_cell::{ConstStaticCell, StaticCell};

esp_bootloader_esp_idf::esp_app_desc!();

#[embassy_executor::task]
async fn display_task(peripherals: board::DisplayPeripherals) {
    #[cfg(feature = "usb-diagnostics")]
    diagnostics::STAGE.store(1, core::sync::atomic::Ordering::Relaxed);
    if let Some(panel) = board::init(peripherals).await {
        #[cfg(feature = "usb-diagnostics")]
        diagnostics::STAGE.store(2, core::sync::atomic::Ordering::Relaxed);
        log::info!(
            "PocketJS QuickJS GUI on core {}",
            esp_hal::system::Cpu::current() as usize
        );
        #[cfg(feature = "usb-diagnostics")]
        diagnostics::benchmark_memory();
        if let Err(error) = display::run(panel).await {
            log::error!("QuickJS initialization failed: {error:?}");
        }
    }
}

#[esp_rtos::main]
async fn main(_spawner: Spawner) {
    #[cfg(not(feature = "usb-diagnostics"))]
    esp_println::logger::init_logger_from_env();
    #[cfg(feature = "usb-diagnostics")]
    {
        log::set_logger(&diagnostics::LOGGER).unwrap();
        log::set_max_level(log::LevelFilter::Info);
    }
    let p = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    let psram = {
        let psram = esp_hal::psram::Psram::new(p.PSRAM, Default::default());
        let (start, size) = psram.raw_parts();
        let framebuffer_bytes = board::SCREEN_W * board::SCREEN_H * 2;
        assert!(size > framebuffer_bytes);
        log::info!(
            "PSRAM={} bytes; GUI heap={} bytes",
            size,
            size - framebuffer_bytes
        );
        // Keep the GUI heap separate from the radio controller's C allocator.
        // SAFETY: the panel owns the first framebuffer; this nonoverlapping tail lives for the firmware lifetime.
        unsafe {
            allocation::GUI_HEAP.add_region(esp_alloc::HeapRegion::new(
                start.add(framebuffer_bytes),
                size - framebuffer_bytes,
                esp_alloc::MemoryCapability::External.into(),
            ));
        }
        psram
    };
    // The pinned S31 linker exposes 64 KiB of boot RAM after startup.
    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 64 * 1024);
    esp_alloc::heap_allocator!(size: 8 * 1024);
    let timg0 = TimerGroup::new(p.TIMG0);
    esp_rtos::start(timg0.timer0);

    let display_peripherals = board::DisplayPeripherals {
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

    let connector = BleConnector::new(p.BT, Default::default()).unwrap();
    let controller: ExternalController<_, 64> = ExternalController::new(connector);
    log::info!(
        "BLE initialized: internal heap free={} bytes",
        esp_alloc::HEAP.free()
    );
    let mut address: [u8; 6] = esp_hal::efuse::base_mac_address()
        .as_bytes()
        .try_into()
        .unwrap();
    address.reverse();
    address[5] |= 0xc0;

    const USB_RX_BYTES: usize = if cfg!(feature = "rynk") { 1536 } else { 1024 };
    static EP_OUT_BUFFER: StaticCell<[u8; USB_RX_BYTES]> = StaticCell::new();
    let usb = Usb::new_hs(p.USB_HS);
    let driver = Driver::new(
        usb,
        EP_OUT_BUFFER.init([0; USB_RX_BYTES]),
        Config::default(),
    );

    let device_config = DeviceConfig {
        vid: 0x4c4b,
        pid: 0x4644,
        manufacturer: "RMK",
        product_name: "RMK Mosaico Dongle",
        ..Default::default()
    };
    let storage_config = StorageConfig {
        start_addr: 0xff0000,
        num_sectors: 16,
        ..Default::default()
    };
    let flash = async_flash_wrapper(FlashStorage::new(p.FLASH).multicore_auto_park());
    let mut storage = new_storage_without_keymap(flash, storage_config).await;

    let router = DongleRouter::new();
    let mut dongle = Dongle::new(controller, address, &router);
    static BOOT_HANDLER: StaticCell<usb_boot::BootloaderHandler> = StaticCell::new();
    let mut builder = UsbTransport::builder(driver, device_config);
    builder
        .usb_builder()
        .handler(BOOT_HANDLER.init(usb_boot::BootloaderHandler));
    let reload_endpoint = {
        let mut function = builder.usb_builder().function(0xff, 0x4d, 1);
        let mut interface = function.interface();
        let mut alternate = interface.alt_setting(0xff, 0x4d, 1, None);
        alternate.endpoint_bulk_out(None, reload::PACKET_SIZE as u16)
    };
    let mut usb = builder.build().with_dongle_router(&router);
    // Keep display rendering and SPI wakeups independent of the USB/BLE executor.
    const DISPLAY_STACK_SIZE: usize = pocketjs_embassy::JS_STACK_LIMIT + 64 * 1024;
    static DISPLAY_STACK: ConstStaticCell<Stack<DISPLAY_STACK_SIZE>> =
        ConstStaticCell::new(Stack::new());
    static DISPLAY_EXECUTOR: StaticCell<esp_rtos::embassy::Executor> = StaticCell::new();
    esp_rtos::start_second_core(p.CPU_CTRL, DISPLAY_STACK.take(), move || {
        let executor = DISPLAY_EXECUTOR.init(esp_rtos::embassy::Executor::new());
        executor.run(|spawner| {
            spawner.spawn(display_task(display_peripherals).unwrap());
        });
    });
    let mut display_state = display::StateCollector::default();
    let boot_task = async {
        usb_boot::REQUESTED.wait().await;
        // Let the control transfer finish before disconnecting the running USB device.
        embassy_time::Timer::after_millis(100).await;
        rmk::boot::jump_to_bootloader();
    };
    embassy_futures::join::join3(
        async { run_all!(usb, dongle, storage, display_state).await },
        boot_task,
        reload::receive_bulk(reload_endpoint),
    )
    .await;
}
