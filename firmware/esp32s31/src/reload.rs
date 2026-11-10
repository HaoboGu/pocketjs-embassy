//! USB only queues bounded packets; allocation and JS execution stay on Core 1.
use core::cell::RefCell;
use embassy_sync::{
    blocking_mutex::{Mutex, raw::CriticalSectionRawMutex},
    channel::Channel,
};
use pocketjs_embassy::reload::{ABI, Bundle, MAX_BUNDLE_BYTES, Upload, UploadError};

pub const REQUEST: u8 = 0x75;
pub const MAGIC: u16 = 0x4d53;
pub const PACKET_SIZE: usize = 512;
pub struct Packet {
    operation: u16,
    length: usize,
    bytes: [u8; PACKET_SIZE],
}
pub static PACKETS: Channel<CriticalSectionRawMutex, Packet, 1> = Channel::new();

struct Status {
    phase: u32,
    received: u32,
    total: u32,
    generation: u32,
    checksum: u32,
    error: u32,
    message: heapless::String<128>,
}
static STATUS: Mutex<CriticalSectionRawMutex, RefCell<Status>> = Mutex::new(RefCell::new(Status {
    phase: 0,
    received: 0,
    total: 0,
    generation: 0,
    checksum: 0,
    error: 0,
    message: heapless::String::new(),
}));

pub fn queue(operation: u16, data: &[u8]) -> bool {
    let valid = match operation {
        0 => data.len() == 20,
        2 | 3 => data.is_empty(),
        _ => false,
    };
    if !valid {
        return false;
    }
    let mut packet = Packet {
        operation,
        length: data.len(),
        bytes: [0; PACKET_SIZE],
    };
    packet.bytes[..data.len()].copy_from_slice(data);
    PACKETS.try_send(packet).is_ok()
}

pub async fn receive_bulk(mut endpoint: impl embassy_usb::driver::EndpointOut) {
    use embassy_usb::driver::EndpointError;
    loop {
        endpoint.wait_enabled().await;
        loop {
            let mut packet = Packet {
                operation: 1,
                length: 0,
                bytes: [0; PACKET_SIZE],
            };
            match endpoint.read(&mut packet.bytes).await {
                Ok(length) if length > 4 => {
                    packet.length = length;
                    PACKETS.send(packet).await;
                }
                Ok(0) => {}
                Ok(_) => failed(2, "Bulk packet is missing its offset or payload"),
                Err(EndpointError::Disabled) => break,
                Err(EndpointError::BufferOverflow) => failed(2, "Oversized bulk packet"),
            }
        }
    }
}

pub fn read(index: u16, out: &mut [u8]) -> usize {
    STATUS.lock(|cell| {
        let status = cell.borrow();
        if index == 1 {
            let len = out.len().min(status.message.len());
            out[..len].copy_from_slice(&status.message.as_bytes()[..len]);
            return len;
        }
        if index != 0 {
            return 0;
        }
        let words = [
            u32::from_le_bytes(*b"MUI1"),
            2,
            ABI,
            (PACKET_SIZE - 4) as u32,
            MAX_BUNDLE_BYTES as u32,
            status.phase,
            status.received,
            status.total,
            status.generation,
            status.checksum,
            status.error,
            embassy_time::Instant::now().as_millis() as u32,
        ];
        let mut bytes = [0u8; 48];
        for (dst, word) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(words) {
            dst.copy_from_slice(&word.to_le_bytes());
        }
        let len = out.len().min(bytes.len());
        out[..len].copy_from_slice(&bytes[..len]);
        len
    })
}

pub fn failed(code: u32, message: &str) {
    STATUS.lock(|cell| {
        let mut status = cell.borrow_mut();
        status.phase = 4;
        status.error = code;
        status.message.clear();
        for ch in message.chars() {
            if status.message.push(ch).is_err() {
                break;
            }
        }
    });
}
pub fn applied(checksum: u32) {
    STATUS.lock(|cell| {
        let mut status = cell.borrow_mut();
        status.phase = 3;
        status.generation = status.generation.wrapping_add(1);
        status.checksum = checksum;
        status.error = 0;
        status.message.clear();
    });
}

pub struct Transfer {
    upload: Option<Upload>,
}
impl Transfer {
    pub fn new() -> Self {
        Self { upload: None }
    }
    pub fn process(&mut self, packet: Packet) -> Result<Option<Bundle>, UploadError> {
        let bytes = &packet.bytes[..packet.length];
        match packet.operation {
            0 => {
                self.upload = None;
                let upload = Upload::begin(bytes)?;
                STATUS.lock(|cell| {
                    let mut status = cell.borrow_mut();
                    status.phase = 1;
                    status.received = 0;
                    status.total = upload.total() as u32;
                    status.error = 0;
                    status.message.clear();
                });
                self.upload = Some(upload);
            }
            1 => {
                let upload = self.upload.as_mut().ok_or(UploadError::NotStarted)?;
                let offset = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
                if let Err(error) = upload.append(offset, &bytes[4..]) {
                    extern crate alloc;
                    failed(
                        error as u32,
                        &alloc::format!(
                            "Upload offset {offset}, expected {}, payload {} bytes",
                            upload.received(),
                            bytes.len() - 4
                        ),
                    );
                    return Ok(None);
                }
                STATUS.lock(|cell| cell.borrow_mut().received = upload.received() as u32);
            }
            2 => {
                let bundle = self
                    .upload
                    .take()
                    .ok_or(UploadError::NotStarted)?
                    .finish()?;
                STATUS.lock(|cell| cell.borrow_mut().phase = 2);
                return Ok(Some(bundle));
            }
            3 => {
                self.upload = None;
                STATUS.lock(|cell| {
                    let mut status = cell.borrow_mut();
                    status.phase = 0;
                    status.received = 0;
                    status.total = 0;
                    status.error = 0;
                    status.message.clear();
                });
            }
            _ => unreachable!("USB boundary validates operations"),
        }
        Ok(None)
    }
}
