use core::{
    cell::RefCell,
    fmt::Write,
    sync::atomic::{AtomicU32, Ordering},
};
use embassy_sync::blocking_mutex::{Mutex, raw::CriticalSectionRawMutex};

const TRACE_CAPACITY: usize = 512;

struct Trace {
    bytes: [u8; TRACE_CAPACITY],
    used: usize,
}
static TRACE: Mutex<CriticalSectionRawMutex, RefCell<Trace>> = Mutex::new(RefCell::new(Trace {
    bytes: [0; TRACE_CAPACITY],
    used: 0,
}));
pub static STAGE: AtomicU32 = AtomicU32::new(0);
pub static FRAMES: AtomicU32 = AtomicU32::new(0);
static PANICS: AtomicU32 = AtomicU32::new(0);
pub struct TraceLogger;
pub static LOGGER: TraceLogger = TraceLogger;

pub fn append(bytes: &[u8]) {
    TRACE.lock(|cell| {
        let mut trace = cell.borrow_mut();
        let count = bytes.len().min(trace.bytes.len());
        let keep = trace.used.min(trace.bytes.len() - count);
        let previous = trace.used;
        if keep < previous {
            trace.bytes.copy_within(previous - keep..previous, 0);
        }
        trace.bytes[keep..keep + count].copy_from_slice(&bytes[bytes.len() - count..]);
        trace.used = keep + count;
    });
}

impl log::Log for TraceLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Info
    }
    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let mut line = heapless::String::<320>::new();
        let _ = writeln!(
            line,
            "[{} c{}] {}",
            record.level(),
            esp_hal::system::Cpu::current() as usize,
            record.args()
        );
        append(line.as_bytes());
    }
    fn flush(&self) {}
}

#[unsafe(no_mangle)]
extern "Rust" fn custom_pre_backtrace() {
    PANICS.fetch_or(
        1 << esp_hal::system::Cpu::current() as usize,
        Ordering::Relaxed,
    );
    let mut line = heapless::String::<256>::new();
    let cause: usize;
    let pc: usize;
    let value: usize;
    // Read the current machine trap record without touching shared peripherals.
    unsafe {
        core::arch::asm!("csrr {0}, mcause", out(reg) cause);
        core::arch::asm!("csrr {0}, mepc", out(reg) pc);
        core::arch::asm!("csrr {0}, mtval", out(reg) value);
    }
    let _ = writeln!(
        line,
        "PANIC c{} mcause={cause:#x} mepc={pc:#x} mtval={value:#x}",
        esp_hal::system::Cpu::current() as usize
    );
    append(line.as_bytes());
    line.clear();
    for frame in esp_backtrace::Backtrace::capture().frames() {
        let _ = write!(line, "{:#x} ", frame.program_counter());
    }
    let _ = writeln!(line);
    append(line.as_bytes());
}

#[cfg(feature = "frame-benchmark")]
extern crate alloc;
#[cfg(feature = "frame-benchmark")]
use core::sync::atomic::AtomicBool;

#[cfg(feature = "frame-benchmark")]
pub static MEASURING: AtomicBool = AtomicBool::new(false);
#[cfg(feature = "frame-benchmark")]
struct FrameProfile {
    last_us: u32,
    phases_us: [u32; 6],
}
#[cfg(feature = "frame-benchmark")]
static PROFILE: Mutex<CriticalSectionRawMutex, RefCell<FrameProfile>> =
    Mutex::new(RefCell::new(FrameProfile {
        last_us: 0,
        phases_us: [0; 6],
    }));
#[cfg(feature = "frame-benchmark")]
static PRESENTS: Mutex<CriticalSectionRawMutex, RefCell<alloc::vec::Vec<[u32; 14]>>> =
    Mutex::new(RefCell::new(alloc::vec::Vec::new()));

#[cfg(feature = "frame-benchmark")]
#[unsafe(no_mangle)]
extern "C" fn pocket_frame_phase(phase: u32) {
    if !MEASURING.load(Ordering::Relaxed) {
        return;
    }
    let now = embassy_time::Instant::now().as_micros() as u32;
    PROFILE.lock(|cell| {
        let mut profile = cell.borrow_mut();
        if phase != 0 {
            profile.phases_us[phase as usize - 1] = now.wrapping_sub(profile.last_us);
        }
        profile.last_us = now;
    });
}

#[cfg(feature = "frame-benchmark")]
pub fn record_present(row: [u32; 8]) {
    let phases = PROFILE.lock(|cell| cell.borrow().phases_us);
    let mut complete = [0; 14];
    complete[..8].copy_from_slice(&row);
    complete[8..].copy_from_slice(&phases);
    PRESENTS.lock(|cell| cell.borrow_mut().push(complete));
}

#[cfg(feature = "frame-benchmark")]
pub fn prepare_presents() {
    let rows = alloc::vec::Vec::with_capacity(if cfg!(feature = "ppa-render") {
        8 * 192
    } else {
        4 * 192
    });
    PRESENTS.lock(|cell| *cell.borrow_mut() = rows);
}

pub fn read(value: u16, index: u16, out: &mut [u8]) -> Option<usize> {
    match value {
        0x4d53 => {
            let fields = [
                u32::from_le_bytes(*b"QJD1"),
                TRACE.lock(|cell| cell.borrow().used as u32),
                PANICS.load(Ordering::Relaxed),
                STAGE.load(Ordering::Relaxed),
                FRAMES.load(Ordering::Relaxed),
                esp_alloc::HEAP.free() as u32,
                crate::allocation::GUI_HEAP.free() as u32,
            ];
            let mut bytes = [0; 28];
            for (slot, value) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(fields) {
                slot.copy_from_slice(&value.to_le_bytes());
            }
            let length = out.len().min(bytes.len());
            out[..length].copy_from_slice(&bytes[..length]);
            Some(length)
        }
        0x4d54 => TRACE.lock(|cell| {
            let trace = cell.borrow();
            let start = usize::from(index).min(trace.used);
            let length = out.len().min(trace.used - start);
            out[..length].copy_from_slice(&trace.bytes[start..start + length]);
            Some(length)
        }),
        #[cfg(feature = "frame-benchmark")]
        0x4d5a => PRESENTS.lock(|cell| {
            let rows = cell.borrow();
            if index == u16::MAX {
                let bytes = (rows.len() as u32).to_le_bytes();
                let length = out.len().min(4);
                out[..length].copy_from_slice(&bytes[..length]);
                return Some(length);
            }
            let row = rows.get(usize::from(index))?;
            let mut bytes = [0; 56];
            for (slot, value) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(row) {
                slot.copy_from_slice(&value.to_le_bytes());
            }
            let length = out.len().min(bytes.len());
            out[..length].copy_from_slice(&bytes[..length]);
            Some(length)
        }),
        #[cfg(feature = "ppa-probe")]
        0x4d5b => Some(crate::acceleration::read(index, out)),
        _ => None,
    }
}
