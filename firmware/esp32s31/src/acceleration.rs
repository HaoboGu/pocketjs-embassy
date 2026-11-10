//! ESP32-S31 PPA bring-up, following ESP-IDF 0e6b480f858c DMA2D/PPA LL.
extern crate alloc;

#[cfg(feature = "ppa-probe")]
use alloc::vec::Vec;
use core::{
    cell::UnsafeCell,
    future::poll_fn,
    sync::atomic::{AtomicU32, Ordering, fence},
    task::Poll,
};
use embassy_futures::select::{Either, select};
use embassy_sync::waitqueue::AtomicWaker;
use embassy_time::{Instant, Timer};
use esp_hal::dma::aligned::{DmaAlignedMut, DmaAlignedRef};
use esp_hal::interrupt;
use esp32s31_pac as pac;
use static_cell::StaticCell;
#[cfg(feature = "ppa-probe")]
use {
    core::cell::RefCell,
    embassy_sync::blocking_mutex::{Mutex, raw::CriticalSectionRawMutex},
};

const WIDTH: usize = 480;
const HEIGHT: usize = 480;
const EVENTS: u32 = 0x0e;
const DESCRIPTOR_COUNT: usize = if cfg!(feature = "ppa-probe") { 3 } else { 1 };
static COMPLETE: AtomicU32 = AtomicU32::new(0);
static WAKER: AtomicWaker = AtomicWaker::new();
#[cfg(feature = "ppa-probe")]
static ROWS: Mutex<CriticalSectionRawMutex, RefCell<Vec<[u32; 13]>>> =
    Mutex::new(RefCell::new(Vec::new()));

#[repr(C, align(64))]
struct Descriptor {
    words: UnsafeCell<[u32; 16]>,
}
#[cfg(feature = "ppa-probe")]
#[repr(C, align(64))]
#[derive(Clone, Copy)]
struct Pixels([u16; 32]);
#[cfg(feature = "ppa-probe")]
#[repr(C, align(64))]
#[derive(Clone, Copy)]
struct ArgbPixels([u32; 16]);

#[esp_hal::handler]
fn dma_interrupt() {
    let dma = unsafe { &*pac::DMA2D::ptr() };
    let status = dma.in_int_raw_ch(0).read().bits() & EVENTS;
    if status != 0 {
        unsafe {
            dma.in_int_ena_ch(0).write(|w| w.bits(0));
            dma.in_int_clr_ch(0).write(|w| w.bits(status));
        }
        COMPLETE.store(status, Ordering::Release);
        WAKER.wake();
    }
}

struct Ppa {
    ppa: pac::PPA,
    dma: pac::DMA2D,
    descriptors: &'static mut [Descriptor; DESCRIPTOR_COUNT],
}

impl Ppa {
    // This board does not use PPA/DMA2D elsewhere; SPI owns the separate AXI DMA.
    fn new() -> Self {
        static DESCRIPTORS: StaticCell<[Descriptor; DESCRIPTOR_COUNT]> = StaticCell::new();
        let ppa = unsafe { pac::PPA::steal() };
        let dma = unsafe { pac::DMA2D::steal() };
        let clock = unsafe { &*pac::HP_SYS_CLKRST::ptr() };
        let system = unsafe { &*pac::HP_SYS::ptr() };
        unsafe {
            clock
                .ppa_ctrl0()
                .modify(|_, w| w.sys_clk_en().set_bit().rst_en().set_bit());
            clock.ppa_ctrl0().modify(|_, w| w.rst_en().clear_bit());
            clock
                .dma2d_ctrl0()
                .modify(|_, w| w.sys_clk_en().set_bit().rst_en().set_bit());
            clock.dma2d_ctrl0().modify(|_, w| w.rst_en().clear_bit());
            system.ppa_mem_lp_ctrl().modify(|_, w| {
                w.ppa_sprf_mem_force_ctrl()
                    .set_bit()
                    .ppa_sprf_mem_lp_en()
                    .clear_bit()
            });
            system.dma2d_mem_lp_ctrl().modify(|_, w| {
                w._2ddma_mem_force_ctrl()
                    .set_bit()
                    ._2ddma_mem_lp_en()
                    .clear_bit()
            });
            dma.in_int_ena_ch(0).write(|w| w.bits(0));
            dma.in_int_clr_ch(0).write(|w| w.bits(u32::MAX));
            interrupt::bind_handler(pac::Interrupt::DMA2D_IN_CH0, dma_interrupt);
        }
        let descriptors = DESCRIPTORS.init(
            [const {
                Descriptor {
                    words: UnsafeCell::new([0; 16]),
                }
            }; DESCRIPTOR_COUNT],
        );
        Self {
            ppa,
            dma,
            descriptors,
        }
    }

    async fn fill(
        &mut self,
        pixels: &mut [u16],
        x: usize,
        y: usize,
        width: usize,
        height: usize,
        color: u16,
    ) -> Result<[u32; 3], u32> {
        self.paint(pixels, (x, y, width, height), color, None).await
    }

    async fn paint(
        &mut self,
        pixels: &mut [u16],
        area: (usize, usize, usize, usize),
        color: u16,
        foreground: Option<&[u32]>,
    ) -> Result<[u32; 3], u32> {
        let (x, y, width, height) = area;
        if pixels.len() != WIDTH * HEIGHT
            || !(pixels.as_ptr() as usize).is_multiple_of(64)
            || width == 0
            || height == 0
            || x + width > WIDTH
            || y + height > HEIGHT
            || !x.is_multiple_of(2)
            || !width.is_multiple_of(2)
        {
            return Err(1);
        }
        if foreground.is_some_and(|fg| {
            fg.len() != WIDTH * HEIGHT || !(fg.as_ptr() as usize).is_multiple_of(64)
        }) {
            return Err(1);
        }
        let start = Instant::now();
        if let Some(foreground) = foreground {
            DmaAlignedRef::new(&foreground[y * WIDTH..(y + height) * WIDTH])
                .map_err(|_| 1u32)?
                .writeback();
        }
        let address = pixels.as_mut_ptr() as u32;
        let mut window =
            DmaAlignedMut::new(&mut pixels[y * WIDTH..(y + height) * WIDTH]).map_err(|_| 1u32)?;
        window.writeback();
        window.invalidate();
        unsafe {
            self.ppa
                .blend_trans_mode()
                .modify(|_, w| w.blend_rst().set_bit());
            self.ppa
                .blend_trans_mode()
                .modify(|_, w| w.blend_rst().clear_bit());
            self.dma.in_int_ena_ch(0).write(|w| w.bits(0));
            self.dma
                .in_conf0_ch(0)
                .modify(|_, w| w.in_cmd_disable_ch().set_bit());
        }
        let reset_deadline = Instant::now() + embassy_time::Duration::from_millis(10);
        while !self.dma.in_state_ch(0).read().in_reset_avail_ch().bit() {
            if Instant::now() >= reset_deadline {
                self.reset();
                return Err(2);
            }
            embassy_futures::yield_now().await;
        }
        if let Some(foreground) = foreground {
            for (channel, (source, pbyte)) in [(address, 3u32), (foreground.as_ptr() as u32, 5)]
                .into_iter()
                .enumerate()
            {
                self.dma
                    .out_conf0_ch(channel)
                    .modify(|_, w| w.out_cmd_disable_ch().set_bit());
                let deadline = Instant::now() + embassy_time::Duration::from_millis(10);
                while !self
                    .dma
                    .out_state_ch(channel)
                    .read()
                    .out_reset_avail_ch()
                    .bit()
                {
                    if Instant::now() >= deadline {
                        self.reset();
                        return Err(2);
                    }
                    embassy_futures::yield_now().await;
                }
                let descriptor = self.descriptors[channel + 1].words.get();
                let mut words = [0u32; 16];
                words[0] = height as u32 | (width as u32) << 14 | 7 << 29;
                words[1] = HEIGHT as u32 | (WIDTH as u32) << 14 | pbyte << 28;
                words[2] = y as u32 | (x as u32) << 14;
                words[3] = source;
                unsafe {
                    descriptor.write_volatile(words);
                    DmaAlignedMut::new(&mut self.descriptors[channel + 1])
                        .unwrap()
                        .writeback();
                    self.dma
                        .out_conf0_ch(channel)
                        .modify(|_, w| w.out_rst_ch().set_bit());
                    self.dma.out_conf0_ch(channel).write(|w| {
                        w.out_eof_mode_ch()
                            .set_bit()
                            .outdscr_burst_en_ch()
                            .set_bit()
                            .out_page_bound_en_ch()
                            .set_bit()
                            .out_macro_block_size_ch()
                            .bits(3)
                            .out_mem_burst_length_ch()
                            .bits(3)
                    });
                    self.dma.out_color_convert_ch(channel).write(|w| {
                        w.out_color_input_sel_ch()
                            .bits(7)
                            .out_color_output_sel_ch()
                            .bits(2)
                    });
                    self.dma
                        .out_peri_sel_ch(channel)
                        .write(|w| w.out_peri_sel_ch().bits(channel as u8 + 2));
                    self.dma
                        .out_link_addr_ch(channel)
                        .write(|w| w.outlink_addr_ch().bits(descriptor as u32));
                    self.dma.out_int_ena_ch(channel).write(|w| w.bits(0));
                    self.dma.out_int_clr_ch(channel).write(|w| w.bits(u32::MAX));
                }
            }
        }
        let descriptor = self.descriptors[0].words.get();
        let mut words = [0u32; 16];
        words[0] = height as u32 | (width as u32) << 14 | 7 << 29;
        words[1] = HEIGHT as u32 | (WIDTH as u32) << 14 | 3 << 28;
        words[2] = y as u32 | (x as u32) << 14;
        words[3] = address;
        unsafe {
            descriptor.write_volatile(words);
            DmaAlignedMut::new(&mut self.descriptors[0])
                .unwrap()
                .writeback();
            self.dma
                .in_conf0_ch(0)
                .modify(|_, w| w.in_rst_ch().set_bit());
            self.dma.in_conf0_ch(0).write(|w| {
                w.indscr_burst_en_ch()
                    .set_bit()
                    .in_page_bound_en_ch()
                    .set_bit()
                    .in_macro_block_size_ch()
                    .bits(3)
                    .in_mem_burst_length_ch()
                    .bits(3)
            });
            self.dma.in_color_convert_ch0().write(|w| {
                w.in_color_input_sel_ch()
                    .bits(7)
                    .in_color_output_sel_ch()
                    .bits(1)
            });
            self.dma
                .in_peri_sel_ch(0)
                .write(|w| w.in_peri_sel_ch().bits(2));
            self.dma
                .in_link_addr_ch(0)
                .write(|w| w.inlink_addr_ch().bits(descriptor as u32));
            self.dma.in_int_clr_ch(0).write(|w| w.bits(u32::MAX));
            COMPLETE.store(0, Ordering::Relaxed);
            self.dma.in_int_ena_ch(0).write(|w| w.bits(EVENTS));
            // This S31 converts the fill register's RGB888 into the output format.
            // Replicate the RGB565 bits so conversion returns the exact input pixel.
            let r = (color as u32 >> 11) & 31;
            let g = (color as u32 >> 5) & 63;
            let b = color as u32 & 31;
            let argb = 0xff00_0000
                | ((r << 3 | r >> 2) << 16)
                | ((g << 2 | g >> 4) << 8)
                | (b << 3 | b >> 2);
            self.ppa.blend_fix_pixel().write(|w| w.bits(argb));
            self.ppa.blend_color_mode().write(|w| {
                w.blend_tx_cm()
                    .bits(2)
                    .blend0_rx_cm()
                    .bits(if foreground.is_some() { 2 } else { 0 })
                    .blend1_rx_cm()
                    .bits(0)
            });
            self.ppa.blend_byte_order().write(|w| w.bits(0));
            self.ppa.blend_fix_alpha().write(|w| w.bits(0));
            self.ppa.ck_bg_low().write(|w| w.bits(0x00ff_ffff));
            self.ppa.ck_bg_high().write(|w| w.bits(0));
            self.ppa.ck_fg_low().write(|w| w.bits(0x00ff_ffff));
            self.ppa.ck_fg_high().write(|w| w.bits(0));
            self.ppa.ck_default().write(|w| w.bits(0));
            self.ppa.blend_tx_size().write(|w| {
                w.blend_hb()
                    .bits(width as u16)
                    .blend_vb()
                    .bits(height as u16)
            });
            fence(Ordering::SeqCst);
            if foreground.is_some() {
                for channel in 0..2 {
                    self.dma
                        .out_link_conf_ch(channel)
                        .write(|w| w.outlink_start_ch().set_bit());
                }
            }
            self.dma
                .in_link_conf_ch(0)
                .write(|w| w.inlink_start_ch().set_bit());
            self.ppa
                .blend_trans_mode()
                .write(|w| w.blend_fix_pixel_fill_en().bit(foreground.is_none()));
            self.ppa
                .blend_trans_mode()
                .modify(|_, w| w.blend_en().set_bit());
            self.ppa
                .blend_trans_mode()
                .modify(|_, w| w.update().set_bit());
        }
        let submitted = Instant::now();
        let wait = poll_fn(|cx| {
            WAKER.register(cx.waker());
            let status = COMPLETE.load(Ordering::Acquire);
            if status == 0 {
                Poll::Pending
            } else {
                Poll::Ready(status)
            }
        });
        let status = match select(wait, Timer::after_millis(100)).await {
            Either::First(status) => status,
            Either::Second(_) => {
                log::error!(
                    "PPA timeout raw={:#x} state={:#x} ppa={:#x}",
                    self.dma.in_int_raw_ch(0).read().bits(),
                    self.dma.in_state_ch(0).read().bits(),
                    self.ppa.blend_st().read().bits()
                );
                log::error!(
                    "PPA cfg mode={:#x} color={:#x} size={:#x} raw={:#x} link={:#x} desc={:#x}",
                    self.ppa.blend_trans_mode().read().bits(),
                    self.ppa.blend_color_mode().read().bits(),
                    self.ppa.blend_tx_size().read().bits(),
                    self.ppa.int_raw().read().bits(),
                    self.dma.in_link_conf_ch(0).read().bits(),
                    descriptor as u32
                );
                DmaAlignedMut::new(&mut self.descriptors[0])
                    .unwrap()
                    .invalidate();
                let words = unsafe { descriptor.read_volatile() };
                log::error!(
                    "PPA desc {:x?} fifo={:#x} conf={:#x}",
                    &words[..5],
                    self.dma.infifo_status_ch(0).read().bits(),
                    self.dma.in_conf0_ch(0).read().bits()
                );
                self.reset();
                window.invalidate();
                log::error!("PPA pixel={:#x}", unsafe {
                    (address as *const u16).add(y * WIDTH + x).read_volatile()
                });
                return Err(3);
            }
        };
        let finished = Instant::now();
        window.invalidate();
        fence(Ordering::SeqCst);
        if status & 0x0c != 0 {
            self.reset();
            return Err(0x100 | status);
        }
        Ok([
            (submitted - start).as_micros() as u32,
            (finished - submitted).as_micros() as u32,
            finished.elapsed().as_micros() as u32,
        ])
    }

    fn reset(&mut self) {
        let clock = unsafe { &*pac::HP_SYS_CLKRST::ptr() };
        unsafe {
            self.dma.in_int_ena_ch(0).write(|w| w.bits(0));
            clock.dma2d_ctrl0().modify(|_, w| w.rst_en().set_bit());
            clock.ppa_ctrl0().modify(|_, w| w.rst_en().set_bit());
            clock.dma2d_ctrl0().modify(|_, w| w.rst_en().clear_bit());
            clock.ppa_ctrl0().modify(|_, w| w.rst_en().clear_bit());
        }
    }
}

#[cfg(feature = "ppa-probe")]
pub async fn probe() {
    let mut hardware = Ppa::new();
    let mut storage = alloc::vec![Pixels([0x1357; 32]); WIDTH * HEIGHT / 32];
    let pixels = unsafe {
        core::slice::from_raw_parts_mut(storage.as_mut_ptr().cast::<u16>(), WIDTH * HEIGHT)
    };
    let mut rows = Vec::with_capacity(12);
    for (width, height) in [
        (8, 8),
        (32, 32),
        (96, 48),
        (208, 96),
        (416, 194),
        (480, 480),
    ] {
        let x = if width == WIDTH { 0 } else { 8 };
        let y = if height == HEIGHT { 0 } else { 8 };
        let count = 24u32;
        let cpu_started = Instant::now();
        for iteration in 0..count {
            let data = core::hint::black_box(&mut *pixels);
            for row in y..y + height {
                data[row * WIDTH + x..row * WIDTH + x + width].fill(0x5a00 + iteration as u16);
            }
        }
        let cpu = cpu_started.elapsed().as_micros() as u32 / count;
        pixels.fill(0x1357);
        let mut totals = [0; 3];
        let mut error = 0;
        let mut completed = 0;
        let started = Instant::now();
        for iteration in 0..count {
            match hardware
                .fill(pixels, x, y, width, height, 0x5a00 + iteration as u16)
                .await
            {
                Ok(times) => {
                    completed += 1;
                    for (sum, time) in totals.iter_mut().zip(times) {
                        *sum += time;
                    }
                }
                Err(code) => {
                    error = code;
                    break;
                }
            }
        }
        let elapsed = started.elapsed().as_micros() as u32 / completed.max(1);
        let mismatches = pixels
            .iter()
            .enumerate()
            .filter(|(index, pixel)| {
                let (px, py) = (index % WIDTH, index / WIDTH);
                let expected = if px >= x && px < x + width && py >= y && py < y + height {
                    0x5a00 + count as u16 - 1
                } else {
                    0x1357
                };
                **pixel != expected
            })
            .count() as u32;
        rows.push([
            width as u32,
            height as u32,
            cpu,
            elapsed,
            totals[0] / count,
            totals[1] / count,
            totals[2] / count,
            mismatches,
            error,
            completed,
            pixels[y * WIDTH + x] as u32,
            0x5a00 + count - 1,
            0,
        ]);
        log::info!(
            "PPA fill {}x{} cpu={}us hardware={}us mismatch={} status={} actual={:#x}",
            width,
            height,
            cpu,
            elapsed,
            mismatches,
            error,
            pixels[y * WIDTH + x]
        );
        if error != 0 {
            break;
        }
    }
    let mut reference_storage = alloc::vec![Pixels([0x1357; 32]); WIDTH * HEIGHT / 32];
    let reference = unsafe {
        core::slice::from_raw_parts_mut(
            reference_storage.as_mut_ptr().cast::<u16>(),
            WIDTH * HEIGHT,
        )
    };
    let mut foreground_storage = alloc::vec![ArgbPixels([0; 16]); WIDTH * HEIGHT / 16];
    let foreground = unsafe {
        core::slice::from_raw_parts_mut(
            foreground_storage.as_mut_ptr().cast::<u32>(),
            WIDTH * HEIGHT,
        )
    };
    for (width, height) in [
        (8, 8),
        (32, 32),
        (96, 48),
        (208, 96),
        (416, 194),
        (480, 480),
    ] {
        let x = if width == WIDTH { 0 } else { 8 };
        let y = if height == HEIGHT { 0 } else { 8 };
        pixels.fill(0x1357);
        reference.fill(0x1357);
        let mut totals = [0u32; 3];
        let (mut cpu, mut hardware_us, mut mismatches, mut error, mut completed) =
            (0u32, 0u32, 0u32, 0u32, 0u32);
        let (mut actual, mut expected) = (0u32, 0u32);
        for iteration in 0..24u32 {
            let alpha = iteration * 17 % 256;
            let (r, g, b) = (
                iteration * 67 % 256,
                iteration * 31 % 256,
                iteration * 137 % 256,
            );
            foreground.fill(alpha << 24 | r << 16 | g << 8 | b);
            let started = Instant::now();
            let target = core::hint::black_box(&mut *reference);
            for row in y..y + height {
                for pixel in &mut target[row * WIDTH + x..row * WIDTH + x + width] {
                    if alpha == 0 {
                        continue;
                    }
                    let dr = (*pixel as u32 >> 11) & 31;
                    let dg = (*pixel as u32 >> 5) & 63;
                    let db = *pixel as u32 & 31;
                    let mix = |s: u32, d: u32| (s * alpha + d * (255 - alpha) + 127) / 255;
                    let r = mix(r, dr << 3 | dr >> 2);
                    let g = mix(g, dg << 2 | dg >> 4);
                    let b = mix(b, db << 3 | db >> 2);
                    *pixel = ((r & 0xf8) << 8 | (g & 0xfc) << 3 | b >> 3) as u16;
                }
            }
            cpu += started.elapsed().as_micros() as u32;
            let started = Instant::now();
            match hardware
                .paint(pixels, (x, y, width, height), 0, Some(foreground))
                .await
            {
                Ok(times) => {
                    for (total, time) in totals.iter_mut().zip(times) {
                        *total += time;
                    }
                    completed += 1;
                }
                Err(code) => {
                    error = code;
                    break;
                }
            }
            hardware_us += started.elapsed().as_micros() as u32;
            let different = pixels
                .iter()
                .zip(reference.iter())
                .filter(|(a, b)| a != b)
                .count() as u32;
            mismatches = mismatches.max(different);
            if different != 0
                && actual == expected
                && let Some((a, b)) = pixels.iter().zip(reference.iter()).find(|(a, b)| a != b)
            {
                actual = *a as u32;
                expected = *b as u32;
            }
        }
        let n = completed.max(1);
        rows.push([
            width as u32,
            height as u32,
            cpu / n,
            hardware_us / n,
            totals[0] / n,
            totals[1] / n,
            totals[2] / n,
            mismatches,
            error,
            completed,
            actual,
            expected,
            1,
        ]);
        log::info!(
            "PPA blend {}x{} cpu={}us hardware={}us mismatch={} status={} actual={:#x} expected={:#x}",
            width,
            height,
            cpu / n,
            hardware_us / n,
            mismatches,
            error,
            actual,
            expected
        );
        if error != 0 {
            break;
        }
    }
    hardware.reset();
    ROWS.lock(|cell| *cell.borrow_mut() = rows);
}

#[cfg(feature = "ppa-probe")]
pub fn read(index: u16, out: &mut [u8]) -> usize {
    ROWS.lock(|cell| {
        let rows = cell.borrow();
        if index == u16::MAX {
            let count = (rows.len() as u32).to_le_bytes();
            let len = out.len().min(4);
            out[..len].copy_from_slice(&count[..len]);
            return len;
        }
        let Some(row) = rows.get(index as usize) else {
            return 0;
        };
        let mut bytes = [0u8; 52];
        for (dst, word) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(row) {
            dst.copy_from_slice(&word.to_le_bytes());
        }
        let len = out.len().min(bytes.len());
        out[..len].copy_from_slice(&bytes[..len]);
        len
    })
}

#[cfg(feature = "ppa-render")]
pub struct Renderer {
    ppa: Ppa,
    pub enabled: bool,
    pub validate: bool,
    pub calls: u32,
    pub pixels: u64,
    pub failures: u32,
    pub mismatched_frames: u32,
    pub verified_frames: u32,
}
#[cfg(feature = "ppa-render")]
impl Renderer {
    pub fn new() -> Self {
        Self {
            ppa: Ppa::new(),
            enabled: true,
            validate: false,
            calls: 0,
            pixels: 0,
            failures: 0,
            mismatched_frames: 0,
            verified_frames: 0,
        }
    }
    async fn fill(
        &mut self,
        pixels: &mut [u16],
        region: pocket_render::damage::DamageRect,
        color: u16,
    ) {
        let (left, right, top, bottom) = (
            region.x0 as usize,
            region.x1 as usize,
            region.y0 as usize,
            region.y1 as usize,
        );
        let x = (left + 1) & !1;
        let end = right & !1;
        let mut completed = false;
        if self.enabled && region.area() >= 8192 && end > x {
            self.calls += 1;
            self.pixels += ((end - x) * (bottom - top)) as u64;
            match self
                .ppa
                .fill(pixels, x, top, end - x, bottom - top, color)
                .await
            {
                Ok(_) => completed = true,
                Err(error) => {
                    self.enabled = false;
                    self.failures += 1;
                    log::error!("PPA renderer disabled: {error}");
                }
            }
        }
        for y in top..bottom {
            let row = y * WIDTH;
            if completed {
                pixels[row + left..row + x].fill(color);
                pixels[row + end..row + right].fill(color);
            } else {
                pixels[row + left..row + right].fill(color);
            }
        }
    }
}
#[cfg(feature = "ppa-render")]
impl pocketjs_embassy::FrameRenderer for Renderer {
    async fn render(
        &mut self,
        resources: &impl pocket_drivers::RenderResources,
        commands: pocket_render::damage::PreparedDrawList<'_>,
        pixels: &mut [u16],
        regions: &[pocket_render::damage::DamageRect],
    ) {
        if !self.enabled
            || self.failures != 0
            || self.mismatched_frames != 0
            || !regions.iter().any(|r| r.area() >= 8192)
        {
            pocket_raster::render_prepared_rgb565_regions(resources, commands, pixels, 1, regions);
            return;
        }
        let mut reference = self.validate.then(|| pixels.to_vec());
        if let Some(reference) = reference.as_mut() {
            pocket_raster::render_prepared_rgb565_regions(
                resources, commands, reference, 1, regions,
            );
        }
        let mut canvas = pocket_raster::Rgb565Canvas::new(resources, pixels, 1);
        for &region in regions {
            if commands.requires_clear(region) {
                canvas.clear(region);
            }
            for command in commands.commands_in(region) {
                let bounds = command.bounds.intersect(region);
                if command.words[0] == pocket_drivers::spec::draw_op::RECT
                    && command.words[3] >> 24 == 255
                    && bounds.area() >= 8192
                {
                    let color = command.words[3];
                    let color = pocket_raster::pack_rgb565(
                        color & 255,
                        color >> 8 & 255,
                        color >> 16 & 255,
                    );
                    self.fill(canvas.pixels_mut(), bounds, color).await;
                } else {
                    canvas.paint(resources, command, region);
                }
            }
        }
        if let Some(reference) = reference {
            if canvas.pixels_mut() != reference {
                self.mismatched_frames += 1;
                self.enabled = false;
                canvas.pixels_mut().copy_from_slice(&reference);
                log::error!("PPA renderer pixel comparison failed; software restored");
            } else {
                self.verified_frames += 1;
            }
        }
    }
}
