// The controller's C allocator keeps esp_alloc::HEAP in internal RAM.
#[cfg_attr(not(feature = "usb-diagnostics"), global_allocator)]
pub static GUI_HEAP: esp_alloc::EspHeap = esp_alloc::EspHeap::empty();

#[cfg(feature = "usb-diagnostics")]
mod measured {
    use core::alloc::{GlobalAlloc, Layout};
    use core::sync::atomic::Ordering;

    struct Allocator;
    #[global_allocator]
    static ALLOCATOR: Allocator = Allocator;

    // All allocations retain GUI_HEAP's ownership and synchronization.
    unsafe impl GlobalAlloc for Allocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            if !crate::diagnostics::MEASURING.load(Ordering::Relaxed) {
                return unsafe { super::GUI_HEAP.alloc(layout) };
            }
            let before: u32;
            unsafe {
                core::arch::asm!("csrr {0}, mcycle", out(reg) before);
            }
            let pointer = unsafe { super::GUI_HEAP.alloc(layout) };
            let after: u32;
            unsafe {
                core::arch::asm!("csrr {0}, mcycle", out(reg) after);
            }
            if esp_hal::system::Cpu::current() == esp_hal::system::Cpu::AppCpu {
                crate::diagnostics::ALLOC_CYCLES
                    .fetch_add(after.wrapping_sub(before), Ordering::Relaxed);
                crate::diagnostics::ALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
            }
            pointer
        }
        unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
            if !crate::diagnostics::MEASURING.load(Ordering::Relaxed) {
                unsafe {
                    super::GUI_HEAP.dealloc(pointer, layout);
                }
                return;
            }
            let before: u32;
            unsafe {
                core::arch::asm!("csrr {0}, mcycle", out(reg) before);
            }
            unsafe {
                super::GUI_HEAP.dealloc(pointer, layout);
            }
            let after: u32;
            unsafe {
                core::arch::asm!("csrr {0}, mcycle", out(reg) after);
            }
            if esp_hal::system::Cpu::current() == esp_hal::system::Cpu::AppCpu {
                crate::diagnostics::ALLOC_CYCLES
                    .fetch_add(after.wrapping_sub(before), Ordering::Relaxed);
                crate::diagnostics::ALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}
