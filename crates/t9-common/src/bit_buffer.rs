use crate::dw_protocol::DwError;

/// Bit-level read/write buffer for DemonWare protocol fields that pack at sub-byte granularity.
pub struct BitBuffer {
    data: Vec<u8>,
    bit_pos: usize,
}

impl BitBuffer {
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            bit_pos: 0,
        }
    }

    pub fn from_bytes(data: Vec<u8>) -> Self {
        Self { data, bit_pos: 0 }
    }

    pub fn get_buffer(&self) -> &[u8] {
        &self.data
    }

    pub fn into_buffer(self) -> Vec<u8> {
        self.data
    }

    pub fn read_bool(&mut self) -> Result<bool, DwError> {
        Ok(self.read_bits(1)? != 0)
    }

    pub fn read_bits(&mut self, count: usize) -> Result<u32, DwError> {
        if count > 32 {
            return Err(DwError::Protocol("bit read > 32".into()));
        }

        let mut result = 0u32;
        for i in 0..count {
            let byte_idx = self.bit_pos / 8;
            let bit_idx = self.bit_pos % 8;
            if byte_idx >= self.data.len() {
                return Err(DwError::BufferUnderflow);
            }
            let bit = ((self.data[byte_idx] >> bit_idx) & 1) as u32;
            result |= bit << i;
            self.bit_pos += 1;
        }
        Ok(result)
    }

    pub fn write_bool(&mut self, val: bool) {
        self.write_bits(if val { 1 } else { 0 }, 1);
    }

    pub fn write_bits(&mut self, val: u32, count: usize) {
        for i in 0..count {
            let byte_idx = self.bit_pos / 8;
            let bit_idx = self.bit_pos % 8;
            while byte_idx >= self.data.len() {
                self.data.push(0);
            }
            let bit = ((val >> i) & 1) as u8;
            self.data[byte_idx] |= bit << bit_idx;
            self.bit_pos += 1;
        }
    }
}

impl Default for BitBuffer {
    fn default() -> Self {
        Self::new()
    }
}
