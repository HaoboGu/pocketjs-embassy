//! ESP-Mosaico CO5300 panel: 480×480 RGB565 over QSPI.

mod window;

use embassy_time::Timer;
use embedded_graphics::prelude::{Point, Size};
use embedded_graphics::primitives::Rectangle;
use esp_hal::dma::DmaTxBuf;
use esp_hal::gpio::{Level, Output, OutputConfig, Pin};
use esp_hal::peripherals::{
    DMA_AXI_CH0, EFUSE, GPIO9, GPIO35, GPIO36, GPIO42, GPIO44, GPIO50, GPIO51, GPIO60, SPI2,
};
use esp_hal::psram::Psram;
use esp_hal::spi::Error;
use esp_hal::spi::master::{Address, Command, Config, DataMode, Spi, SpiDma};
use esp_hal::time::Rate;
use esp_hal::{Async, dma_rx_buffer, dma_tx_buffer};

pub const SCREEN_W: usize = 480;
pub const SCREEN_H: usize = 480;
const PIXELS: usize = SCREEN_W * SCREEN_H;
// Keep each transfer below esp-hal's 32,736-byte DMA limit.
const DMA_CHUNK: usize = 30 * 1024;

pub struct DisplayPeripherals {
    pub spi2: SPI2<'static>,
    pub dma: DMA_AXI_CH0<'static>,
    pub psram: Psram,
    pub power: GPIO60<'static>,
    pub gpio42: GPIO42<'static>,
    pub gpio44: GPIO44<'static>,
    pub data0: GPIO36<'static>,
    pub data1: GPIO51<'static>,
    pub data2: GPIO35<'static>,
    pub data3: GPIO9<'static>,
    pub cs: GPIO50<'static>,
}

pub struct Panel {
    spi: SpiDma<'static, Async>,
    cs: Output<'static>,
    _power: Output<'static>,
    _reset: Output<'static>,
    _psram: Psram,
    // Store RGB565 in wire byte order so flush can copy whole rows.
    framebuffer: &'static mut [u16],
    transfer: [DmaTxBuf; 2],
}

pub async fn init(p: DisplayPeripherals) -> Option<Panel> {
    // The official BSP stores the board version in the first 16 USER_DATA bits.
    let version = EFUSE::regs().rd_usr_data(0).read().bits() as u16;
    let (sck, reset) = match version {
        0x0100 => (p.gpio44.degrade(), p.gpio42.degrade()),
        0x0101 | 0x0102 => (p.gpio42.degrade(), p.gpio44.degrade()),
        _ => {
            log::error!("Unknown Mosaico board version {version:#06x}; display disabled");
            return None;
        }
    };
    log::info!("Mosaico board v{}.{}", version >> 8, version & 0xff);
    let power = Output::new(p.power, Level::Low, OutputConfig::default());
    let mut reset = Output::new(reset, Level::Low, OutputConfig::default());
    Timer::after_millis(10).await;
    reset.set_high();
    Timer::after_millis(150).await;

    let psram = p.psram;
    let (start, _) = psram.raw_parts();
    // SAFETY: the panel owns these first PIXELS * 2 bytes; the GUI heap starts after them.
    let framebuffer = unsafe { core::slice::from_raw_parts_mut(start.cast::<u16>(), PIXELS) };
    framebuffer.fill(0);

    let spi = Spi::new(
        p.spi2,
        Config::default()
            .with_frequency(Rate::from_mhz(40))
            .with_min_async_transfer_size(32),
    )
    .ok()?
    .with_sck(sck)
    .with_sio0(p.data0)
    .with_sio1(p.data1)
    .with_sio2(p.data2)
    .with_sio3(p.data3)
    .with_dma(p.dma)
    .with_buffers(dma_rx_buffer!(1).ok()?, dma_tx_buffer!(16).ok()?)
    .into_async();
    let mut panel = Panel {
        spi,
        cs: Output::new(p.cs, Level::High, OutputConfig::default()),
        _power: power,
        _reset: reset,
        _psram: psram,
        framebuffer,
        transfer: [
            dma_tx_buffer!(DMA_CHUNK).ok()?,
            dma_tx_buffer!(DMA_CHUNK).ok()?,
        ],
    };

    // Initialization and delays match esp-mosaico-bsp's CO5300 vendor sequence.
    panel.command(0x11, &[]).await.ok()?;
    Timer::after_millis(600).await;
    for (command, value) in [
        (0xfe, 0x20),
        (0x19, 0x10),
        (0x1c, 0xa0),
        (0xfe, 0x00),
        (0xc4, 0x80),
        (0x3a, 0x55),
        (0x36, 0x60), // 90 degrees counterclockwise on Mosaico.
        (0x53, 0x20),
        (0x51, 0xff),
        (0x63, 0xff),
    ] {
        panel.command(command, &[value]).await.ok()?;
    }
    panel.command(0x2a, &[0, 0, 1, 0xdf]).await.ok()?;
    panel.command(0x2b, &[0, 0, 1, 0xdf]).await.ok()?;
    panel.command(0x29, &[]).await.ok()?;
    Timer::after_millis(600).await;
    Some(panel)
}

impl Panel {
    async fn command(&mut self, command: u8, data: &[u8]) -> Result<(), Error> {
        self.cs.set_low();
        let result = self
            .spi
            .half_duplex_write_async(
                DataMode::Single,
                Command::_8Bit(0x02, DataMode::Single),
                Address::_24Bit(u32::from(command) << 8, DataMode::Single),
                0,
                data,
            )
            .await;
        self.cs.set_high();
        result
    }

    pub async fn present_rgb565(&mut self, pixels: &[u16], area: Rectangle) -> Result<(), Error> {
        assert_eq!(pixels.len(), PIXELS);
        let bounds = Rectangle::new(Point::zero(), Size::new(SCREEN_W as u32, SCREEN_H as u32));
        let Some(area) = window::aligned_window(area, bounds) else {
            return Ok(());
        };
        for y in area.top_left.y as usize..area.top_left.y as usize + area.size.height as usize {
            let start = y * SCREEN_W + area.top_left.x as usize;
            let end = start + area.size.width as usize;
            for (out, pixel) in self.framebuffer[start..end]
                .iter_mut()
                .zip(&pixels[start..end])
            {
                *out = pixel.to_be();
            }
        }
        let end = area.bottom_right().unwrap();
        let x0 = area.top_left.x as usize;
        let x1 = end.x as usize + 1;
        let width = x1 - x0;
        let y0 = area.top_left.y as usize;
        let height = end.y as usize + 1 - y0;
        let [xs0, xs1] = (x0 as u16).to_be_bytes();
        let [xe0, xe1] = ((x1 - 1) as u16).to_be_bytes();
        let [ys0, ys1] = (y0 as u16).to_be_bytes();
        let [ye0, ye1] = (end.y as u16).to_be_bytes();
        self.command(0x2a, &[xs0, xs1, xe0, xe1]).await?;
        self.command(0x2b, &[ys0, ys1, ye0, ye1]).await?;
        // One RAMWR transaction spans every DMA chunk, as in esp_lcd_panel_io_tx_color.
        self.cs.set_low();
        let region = Rectangle::new(
            Point::new(x0 as i32, y0 as i32),
            Size::new(width as u32, height as u32),
        );
        let mut offset = 0;
        let mut front = 0;
        let mut count = pack_chunk(self.framebuffer, region, offset, &mut self.transfer[front]);
        while count != 0 {
            let (command, address) = if offset == 0 {
                (
                    Command::_8Bit(0x32, DataMode::Single),
                    Address::_24Bit(0x2c00, DataMode::Single),
                )
            } else {
                (Command::None, Address::None)
            };
            let (first, second) = self.transfer.split_at_mut(1);
            let (sending, spare) = if front == 0 {
                (&first[0], &mut second[0])
            } else {
                (&second[0], &mut first[0])
            };
            let next_offset = offset + count;
            // Prepare the next chunk while DMA transmits the current one.
            let (result, next_count) = embassy_futures::join::join(
                self.spi.half_duplex_write_async(
                    DataMode::Quad,
                    command,
                    address,
                    0,
                    &sending.as_slice()[..count * 2],
                ),
                async { pack_chunk(self.framebuffer, region, next_offset, spare) },
            )
            .await;
            if let Err(error) = result {
                self.cs.set_high();
                return Err(error);
            }
            offset = next_offset;
            count = next_count;
            front ^= 1;
        }
        self.cs.set_high();
        Ok(())
    }
}

fn pack_chunk(
    framebuffer: &[u16],
    region: Rectangle,
    offset: usize,
    transfer: &mut DmaTxBuf,
) -> usize {
    let width = region.size.width as usize;
    let count = (DMA_CHUNK / 2).min(width * region.size.height as usize - offset);
    let mut filled = 0;
    while filled < count {
        let pixel = offset + filled;
        let row = region.top_left.y as usize + pixel / width;
        let col = pixel % width;
        let n = (width - col).min(count - filled);
        let start = row * SCREEN_W + region.top_left.x as usize + col;
        // SAFETY: region is clipped to the initialized framebuffer; u8 needs no extra alignment.
        let bytes = unsafe {
            core::slice::from_raw_parts(framebuffer.as_ptr().add(start).cast::<u8>(), n * 2)
        };
        transfer.as_mut_slice()[filled * 2..(filled + n) * 2].copy_from_slice(bytes);
        filled += n;
    }
    count
}
