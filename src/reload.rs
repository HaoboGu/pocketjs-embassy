//! Bounded, sequential upload of a JavaScript bundle and its owned assets.
use alloc::vec::Vec;

pub const MAX_BUNDLE_BYTES: usize = 2 * 1024 * 1024;
pub const ABI: u32 = pocket_drivers::spec::BINDING_GENERATION;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
#[repr(u32)]
pub enum UploadError {
    Header = 1,
    Size = 2,
    Allocation = 3,
    Offset = 4,
    Incomplete = 5,
    Checksum = 6,
    NotStarted = 7,
}

pub struct Bundle {
    pub javascript: Vec<u8>,
    pub assets: Vec<u8>,
    pub checksum: u32,
}

pub struct Upload {
    javascript_len: usize,
    assets_len: usize,
    checksum: u32,
    running_crc: u32,
    received: usize,
    javascript: Vec<u8>,
    assets: Vec<u8>,
}

impl Upload {
    pub fn begin(header: &[u8]) -> Result<Self, UploadError> {
        if header.len() != 20 || &header[..4] != b"MUI1" {
            return Err(UploadError::Header);
        }
        let word = |offset| u32::from_le_bytes(header[offset..offset + 4].try_into().unwrap());
        if word(4) != ABI {
            return Err(UploadError::Header);
        }
        let javascript_len = word(8) as usize;
        let assets_len = word(12) as usize;
        if javascript_len == 0
            || javascript_len > MAX_BUNDLE_BYTES
            || assets_len > MAX_BUNDLE_BYTES - javascript_len
        {
            return Err(UploadError::Size);
        }
        let mut javascript = Vec::new();
        let mut assets = Vec::new();
        javascript
            .try_reserve_exact(javascript_len)
            .map_err(|_| UploadError::Allocation)?;
        assets
            .try_reserve_exact(assets_len)
            .map_err(|_| UploadError::Allocation)?;
        Ok(Self {
            javascript_len,
            assets_len,
            checksum: word(16),
            running_crc: !0,
            received: 0,
            javascript,
            assets,
        })
    }

    pub fn received(&self) -> usize {
        self.received
    }
    pub fn total(&self) -> usize {
        self.javascript_len + self.assets_len
    }

    pub fn append(&mut self, offset: usize, data: &[u8]) -> Result<(), UploadError> {
        if offset != self.received {
            return Err(UploadError::Offset);
        }
        if data.is_empty() || data.len() > self.total() - self.received {
            return Err(UploadError::Size);
        }
        let js_count = self
            .javascript_len
            .saturating_sub(self.received)
            .min(data.len());
        self.javascript.extend_from_slice(&data[..js_count]);
        self.assets.extend_from_slice(&data[js_count..]);
        for &byte in data {
            self.running_crc ^= u32::from(byte);
            for _ in 0..8 {
                self.running_crc = (self.running_crc >> 1)
                    ^ (0xedb8_8320 & 0u32.wrapping_sub(self.running_crc & 1));
            }
        }
        self.received += data.len();
        Ok(())
    }

    pub fn finish(self) -> Result<Bundle, UploadError> {
        if self.received != self.total() {
            return Err(UploadError::Incomplete);
        }
        if !self.running_crc != self.checksum {
            return Err(UploadError::Checksum);
        }
        Ok(Bundle {
            javascript: self.javascript,
            assets: self.assets,
            checksum: self.checksum,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn header() -> Vec<u8> {
        let mut bytes = b"MUI1".to_vec();
        for word in [ABI, 4, 5, 0xcbf4_3926] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes
    }
    #[test]
    fn chunks_can_cross_javascript_and_asset_boundary() {
        let mut upload = Upload::begin(&header()).unwrap();
        upload.append(0, b"123").unwrap();
        upload.append(3, b"456789").unwrap();
        let bundle = upload.finish().unwrap();
        assert_eq!(bundle.javascript, b"1234");
        assert_eq!(bundle.assets, b"56789");
    }
    #[test]
    fn incomplete_corrupt_and_out_of_order_uploads_are_rejected() {
        let mut upload = Upload::begin(&header()).unwrap();
        assert_eq!(upload.append(1, b"123"), Err(UploadError::Offset));
        assert_eq!(upload.received(), 0);
        upload.append(0, b"123").unwrap();
        assert!(matches!(upload.finish(), Err(UploadError::Incomplete)));
        let mut upload = Upload::begin(&header()).unwrap();
        upload.append(0, b"123456780").unwrap();
        assert!(matches!(upload.finish(), Err(UploadError::Checksum)));
    }
    #[test]
    fn size_and_abi_are_checked_before_allocating() {
        let mut bytes = header();
        bytes[4..8].copy_from_slice(&(ABI + 1).to_le_bytes());
        assert!(matches!(Upload::begin(&bytes), Err(UploadError::Header)));
        bytes[4..8].copy_from_slice(&ABI.to_le_bytes());
        bytes[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(Upload::begin(&bytes), Err(UploadError::Size)));
        assert!(matches!(Upload::begin(b"MUI1"), Err(UploadError::Header)));
    }
}
