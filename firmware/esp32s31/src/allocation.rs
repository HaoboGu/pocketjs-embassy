// The controller's C allocator keeps esp_alloc::HEAP in internal RAM.
#[global_allocator]
pub static GUI_HEAP: esp_alloc::EspHeap = esp_alloc::EspHeap::empty();
