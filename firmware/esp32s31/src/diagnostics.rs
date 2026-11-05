use core::{
    cell::RefCell,
    fmt::Write,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
};
use embassy_sync::blocking_mutex::{Mutex, raw::CriticalSectionRawMutex};

struct Trace {
    bytes: [u8; 6144],
    used: usize,
}
static TRACE: Mutex<CriticalSectionRawMutex, RefCell<Trace>> = Mutex::new(RefCell::new(Trace {
    bytes: [0; 6144],
    used: 0,
}));
pub static MEASURING: AtomicBool = AtomicBool::new(true);
pub static STAGE: AtomicU32 = AtomicU32::new(0);
pub static FRAMES: AtomicU32 = AtomicU32::new(0);
pub static ALLOC_CYCLES: AtomicU32 = AtomicU32::new(0);
pub static ALLOC_CALLS: AtomicU32 = AtomicU32::new(0);
pub static INPUT_REVISION: AtomicU32 = AtomicU32::new(0);
pub static APPLIED_REVISION: AtomicU32 = AtomicU32::new(0);
pub static SNAPSHOT_AGE_US: AtomicU32 = AtomicU32::new(0);
pub static APPLY_LATENCY_US: AtomicU32 = AtomicU32::new(0);
static PANICS: AtomicU32 = AtomicU32::new(0);
pub struct TraceLogger;
pub static LOGGER: TraceLogger = TraceLogger;

pub fn append(bytes: &[u8]) {
    TRACE.lock(|cell| {
        let mut trace = cell.borrow_mut();
        let start = trace.used;
        let count = bytes.len().min(trace.bytes.len() - start);
        trace.bytes[start..start + count].copy_from_slice(&bytes[..count]);
        trace.used += count;
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

struct FrameProfile {
    last_us: u32,
    phases_us: [u32; 6],
    completed_us: [u32; 6],
    allocation_us: u32,
    allocation_calls: u32,
}
static PROFILE: Mutex<CriticalSectionRawMutex, RefCell<FrameProfile>> =
    Mutex::new(RefCell::new(FrameProfile {
        last_us: 0,
        phases_us: [0; 6],
        completed_us: [0; 6],
        allocation_us: 0,
        allocation_calls: 0,
    }));

#[unsafe(no_mangle)]
extern "C" fn pocket_frame_phase(phase: u32) {
    if !MEASURING.load(Ordering::Relaxed) {
        return;
    }
    let now = embassy_time::Instant::now().as_micros() as u32;
    if phase == 0 {
        ALLOC_CYCLES.store(0, Ordering::Relaxed);
        ALLOC_CALLS.store(0, Ordering::Relaxed);
    }
    PROFILE.lock(|cell| {
        let mut profile = cell.borrow_mut();
        if phase != 0 {
            profile.phases_us[phase as usize - 1] = now.wrapping_sub(profile.last_us);
        }
        profile.last_us = now;
        if phase == 6 {
            profile.completed_us = profile.phases_us;
            profile.allocation_us = ALLOC_CYCLES.load(Ordering::Relaxed)
                / (esp_hal::clock::cpu_clock().as_hz() / 1_000_000);
            profile.allocation_calls = ALLOC_CALLS.load(Ordering::Relaxed);
        }
    });
}

pub fn read(value: u16, index: u16, out: &mut [u8]) -> usize {
    if value == 0x4d59 {
        FIXTURES.lock(|cell| {
            let fixtures = cell.borrow();
            if index == u16::MAX {
                let bytes = (fixtures.used as u32).to_le_bytes();
                let length = out.len().min(4);
                out[..length].copy_from_slice(&bytes[..length]);
                return length;
            }
            if usize::from(index) >= fixtures.used {
                return 0;
            }
            let mut bytes = [0; 92];
            for (slot, value) in bytes
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(fixtures.rows[usize::from(index)])
            {
                slot.copy_from_slice(&value.to_le_bytes());
            }
            let length = out.len().min(bytes.len());
            out[..length].copy_from_slice(&bytes[..length]);
            length
        })
    } else if value == 0x4d58 {
        let fields = DETAIL.lock(|cell| cell.borrow().completed);
        let mut bytes = [0; 52];
        for (slot, value) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(fields) {
            slot.copy_from_slice(&value.to_le_bytes());
        }
        let length = out.len().min(bytes.len());
        out[..length].copy_from_slice(&bytes[..length]);
        length
    } else if value == 0x4d56 {
        let fields = [
            INPUT_REVISION.load(Ordering::Relaxed),
            APPLIED_REVISION.load(Ordering::Relaxed),
            SNAPSHOT_AGE_US.load(Ordering::Relaxed),
            APPLY_LATENCY_US.load(Ordering::Relaxed),
        ];
        let mut bytes = [0; 16];
        for (slot, value) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(fields) {
            slot.copy_from_slice(&value.to_le_bytes());
        }
        let length = out.len().min(bytes.len());
        out[..length].copy_from_slice(&bytes[..length]);
        length
    } else if value == 0x4d55 {
        let fields = PROFILE.lock(|cell| {
            let profile = cell.borrow();
            let phases = profile.completed_us;
            [
                embassy_time::Instant::now().as_millis() as u32,
                FRAMES.load(Ordering::Relaxed),
                phases[0],
                phases[1],
                phases[2],
                phases[3],
                phases[4],
                phases[5],
                profile.allocation_us,
                profile.allocation_calls,
            ]
        });
        let mut bytes = [0; 40];
        for (slot, value) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(fields) {
            slot.copy_from_slice(&value.to_le_bytes());
        }
        let length = out.len().min(bytes.len());
        out[..length].copy_from_slice(&bytes[..length]);
        length
    } else if value == 0x4d53 {
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
        length
    } else {
        TRACE.lock(|cell| {
            let trace = cell.borrow();
            let start = usize::from(index).min(trace.used);
            let length = out.len().min(trace.used - start);
            out[..length].copy_from_slice(&trace.bytes[start..start + length]);
            length
        })
    }
}

pub fn benchmark_memory() {
    use core::alloc::{GlobalAlloc, Layout};
    let internal = [0x12345678u32; 256];
    let layout = Layout::array::<u32>(internal.len()).unwrap();
    let external = unsafe { crate::allocation::GUI_HEAP.alloc(layout) }.cast::<u32>();
    assert!(!external.is_null());
    unsafe {
        external.copy_from_nonoverlapping(internal.as_ptr(), internal.len());
    }
    for (index, address) in [internal.as_ptr(), external.cast_const()]
        .into_iter()
        .enumerate()
    {
        let started = embassy_time::Instant::now();
        let first_cycle: u32;
        unsafe {
            core::arch::asm!("csrr {0}, mcycle", out(reg) first_cycle);
        }
        let mut sum = 0u32;
        for _ in 0..256 {
            for offset in 0..internal.len() {
                sum = sum.wrapping_add(unsafe { address.add(offset).read_volatile() });
            }
        }
        let last_cycle: u32;
        unsafe {
            core::arch::asm!("csrr {0}, mcycle", out(reg) last_cycle);
        }
        let micros = started.elapsed().as_micros();
        log::info!(
            "Memory probe region={} address={:p} reads=65536 elapsed={}us cycles={} checksum={}",
            index,
            address,
            micros,
            last_cycle.wrapping_sub(first_cycle),
            sum
        );
    }
    unsafe {
        crate::allocation::GUI_HEAP.dealloc(external.cast(), layout);
    }
}

#[derive(Default)]
struct DetailProfile {
    bridge_last: u32,
    app_last: u32,
    guest_last: u32,
    guest_phase: u32,
    bridge: [u32; 4],
    app: [u32; 4],
    js_call_us: u32,
    jobs_us: u32,
    native_us: u32,
    native_calls: u32,
    completed: [u32; 13],
}
static DETAIL: Mutex<CriticalSectionRawMutex, RefCell<DetailProfile>> =
    Mutex::new(RefCell::new(DetailProfile {
        bridge_last: 0,
        app_last: 0,
        guest_last: 0,
        guest_phase: 0,
        bridge: [0; 4],
        app: [0; 4],
        js_call_us: 0,
        jobs_us: 0,
        native_us: 0,
        native_calls: 0,
        completed: [0; 13],
    }));
#[unsafe(no_mangle)]
extern "C" fn pocket_bridge_phase(phase: u32) {
    if !MEASURING.load(Ordering::Relaxed) {
        return;
    }
    let now = embassy_time::Instant::now().as_micros() as u32;
    DETAIL.lock(|cell| {
        let mut detail = cell.borrow_mut();
        if phase == 0 {
            let completed = detail.completed;
            *detail = DetailProfile::default();
            detail.completed = completed;
        } else {
            detail.bridge[phase as usize - 1] = now.wrapping_sub(detail.bridge_last);
        }
        detail.bridge_last = now;
        if phase == 4 {
            detail.completed = [
                now,
                detail.bridge[0],
                detail.bridge[1],
                detail.bridge[2],
                detail.bridge[3],
                detail.app[0],
                detail.app[1],
                detail.app[2],
                detail.app[3],
                detail.js_call_us,
                detail.jobs_us,
                detail.native_us,
                detail.native_calls,
            ];
        }
    });
}
#[unsafe(no_mangle)]
extern "C" fn pocket_app_phase(phase: u32) {
    if !MEASURING.load(Ordering::Relaxed) {
        return;
    }
    let now = embassy_time::Instant::now().as_micros() as u32;
    DETAIL.lock(|cell| {
        let mut detail = cell.borrow_mut();
        if phase != 0 && phase <= 4 {
            detail.app[phase as usize - 1] = now.wrapping_sub(detail.app_last);
        }
        detail.app_last = now;
    });
}
#[unsafe(no_mangle)]
extern "C" fn pocket_guest_phase(phase: u32) {
    if !MEASURING.load(Ordering::Relaxed) {
        return;
    }
    let now = embassy_time::Instant::now().as_micros() as u32;
    DETAIL.lock(|cell| {
        let mut detail = cell.borrow_mut();
        let elapsed = now.wrapping_sub(detail.guest_last);
        match detail.guest_phase {
            2 => detail.js_call_us += elapsed,
            3 => detail.jobs_us += elapsed,
            _ => (),
        }
        detail.guest_phase = phase;
        detail.guest_last = now;
    });
}
#[unsafe(no_mangle)]
extern "C" fn pocket_native_op(micros: u32) {
    if !MEASURING.load(Ordering::Relaxed) {
        return;
    }
    DETAIL.lock(|cell| {
        let mut detail = cell.borrow_mut();
        detail.native_us += micros;
        detail.native_calls += 1;
    });
}

struct Fixtures {
    used: usize,
    rows: [[u32; 23]; 32],
}
static FIXTURES: Mutex<CriticalSectionRawMutex, RefCell<Fixtures>> =
    Mutex::new(RefCell::new(Fixtures {
        used: 0,
        rows: [[0; 23]; 32],
    }));

pub fn report_detail(label: &str, total_us: u64, pixels_hash: u32, cycles: u32) {
    let values = DETAIL.lock(|cell| cell.borrow().completed);
    let native = PROFILE.lock(|cell| cell.borrow().completed_us);
    let index = FIXTURES.lock(|cell| {
        let mut fixtures = cell.borrow_mut();
        let index = fixtures.used;
        if index < fixtures.rows.len() {
            let row = &mut fixtures.rows[index];
            row[0] = u32::from(label == "key_down");
            row[1] = total_us as u32;
            row[2..15].copy_from_slice(&values);
            row[15..21].copy_from_slice(&native);
            row[21] = pixels_hash;
            row[22] = cycles;
            fixtures.used += 1;
        }
        index
    });
    if index >= 6 {
        return;
    }
    log::info!(
        "Frame probe {} total={} serialize={} input={} guest={} status={} calc={} solid={} wake={} framework={} native_ops={} ops={}",
        label,
        total_us,
        values[1],
        values[2],
        values[3],
        values[4],
        values[5],
        values[6],
        values[7],
        values[8],
        values[11],
        values[12]
    );
    log::info!(
        "Frame native {} draw={} damage={} raster={} commit={} js_call={} jobs={}",
        label,
        native[2],
        native[3],
        native[4],
        native[5],
        values[9],
        values[10]
    );
}
