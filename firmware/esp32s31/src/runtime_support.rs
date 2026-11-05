use embassy_time::Instant;

#[unsafe(no_mangle)]
extern "C" fn pocket_uptime_us() -> u64 {
    Instant::now().as_micros()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn pocket_uart_write(bytes: *const u8, length: usize) {
    // Newlib keeps the output buffer alive through this synchronous call.
    let bytes = unsafe { core::slice::from_raw_parts(bytes, length) };
    #[cfg(feature = "usb-diagnostics")]
    crate::diagnostics::append(bytes);
    esp_println::Printer::write_bytes(bytes);
}
