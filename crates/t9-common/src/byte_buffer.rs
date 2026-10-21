use crate::dw_protocol::DwError;

/// Read/write buffer for DemonWare binary protocol.
/// Mirrors the byte_buffer from shield-development.
pub struct ByteBuffer {
    data: Vec<u8>,
    pos: usize,
    use_data_types: bool,
}

impl ByteBuffer {
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            pos: 0,
            use_data_types: true,
        }
    }

    pub fn from_bytes(data: Vec<u8>) -> Self {
        Self {
            data,
            pos: 0,
            use_data_types: true,
        }
    }

    pub fn set_use_data_types(&mut self, use_types: bool) {
        self.use_data_types = use_types;
    }

    pub fn has_more_data(&self) -> bool {
        self.pos < self.data.len()
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    pub fn position(&self) -> usize {
        self.pos
    }

    pub fn get_buffer(&self) -> &[u8] {
        &self.data
    }

    pub fn into_buffer(self) -> Vec<u8> {
        self.data
    }

    pub fn get_remaining(&self) -> &[u8] {
        &self.data[self.pos..]
    }

    fn check_type(&mut self, expected: u8) -> Result<(), DwError> {
        if !self.use_data_types {
            return Ok(());
        }
        let actual = self.read_raw_byte()?;
        if actual != expected {
            return Err(DwError::TypeMismatch { expected, actual });
        }
        Ok(())
    }

    fn write_type(&mut self, type_id: u8) {
        if self.use_data_types {
            self.data.push(type_id);
        }
    }

    fn read_raw_byte(&mut self) -> Result<u8, DwError> {
        if self.pos >= self.data.len() {
            return Err(DwError::BufferUnderflow);
        }
        let val = self.data[self.pos];
        self.pos += 1;
        Ok(val)
    }

    // --- Read methods ---

    pub fn read_bool(&mut self) -> Result<bool, DwError> {
        self.check_type(1)?;
        Ok(self.read_raw_byte()? != 0)
    }

    pub fn read_ubyte(&mut self) -> Result<u8, DwError> {
        self.check_type(3)?;
        self.read_raw_byte()
    }

    pub fn read_byte(&mut self) -> Result<i8, DwError> {
        self.check_type(2)?;
        Ok(self.read_raw_byte()? as i8)
    }

    pub fn read_uint16(&mut self) -> Result<u16, DwError> {
        self.check_type(5)?;
        let b0 = self.read_raw_byte()? as u16;
        let b1 = self.read_raw_byte()? as u16;
        Ok(b0 | (b1 << 8))
    }

    pub fn read_int16(&mut self) -> Result<i16, DwError> {
        self.check_type(4)?;
        let b0 = self.read_raw_byte()? as u16;
        let b1 = self.read_raw_byte()? as u16;
        Ok((b0 | (b1 << 8)) as i16)
    }

    pub fn read_int32(&mut self) -> Result<i32, DwError> {
        self.check_type(6)?;
        let mut bytes = [0u8; 4];
        for b in &mut bytes {
            *b = self.read_raw_byte()?;
        }
        Ok(i32::from_le_bytes(bytes))
    }

    pub fn read_uint32(&mut self) -> Result<u32, DwError> {
        self.check_type(7)?;
        let mut bytes = [0u8; 4];
        for b in &mut bytes {
            *b = self.read_raw_byte()?;
        }
        Ok(u32::from_le_bytes(bytes))
    }

    pub fn read_int64(&mut self) -> Result<i64, DwError> {
        self.check_type(8)?;
        let mut bytes = [0u8; 8];
        for b in &mut bytes {
            *b = self.read_raw_byte()?;
        }
        Ok(i64::from_le_bytes(bytes))
    }

    pub fn read_uint64(&mut self) -> Result<u64, DwError> {
        self.check_type(9)?;
        let mut bytes = [0u8; 8];
        for b in &mut bytes {
            *b = self.read_raw_byte()?;
        }
        Ok(u64::from_le_bytes(bytes))
    }

    pub fn read_float(&mut self) -> Result<f32, DwError> {
        self.check_type(10)?;
        let mut bytes = [0u8; 4];
        for b in &mut bytes {
            *b = self.read_raw_byte()?;
        }
        Ok(f32::from_le_bytes(bytes))
    }

    pub fn read_string(&mut self) -> Result<String, DwError> {
        self.check_type(16)?;
        let len = self.read_raw_uint32()? as usize;
        if self.pos + len > self.data.len() {
            return Err(DwError::BufferUnderflow);
        }
        let s = String::from_utf8_lossy(&self.data[self.pos..self.pos + len]).into_owned();
        self.pos += len;
        Ok(s)
    }

    pub fn read_blob(&mut self) -> Result<Vec<u8>, DwError> {
        self.check_type(13)?;
        let len = self.read_raw_uint32()? as usize;
        if self.pos + len > self.data.len() {
            return Err(DwError::BufferUnderflow);
        }
        let data = self.data[self.pos..self.pos + len].to_vec();
        self.pos += len;
        Ok(data)
    }

    pub fn read_bytes(&mut self, count: usize) -> Result<Vec<u8>, DwError> {
        if self.pos + count > self.data.len() {
            return Err(DwError::BufferUnderflow);
        }
        let data = self.data[self.pos..self.pos + count].to_vec();
        self.pos += count;
        Ok(data)
    }

    fn read_raw_uint32(&mut self) -> Result<u32, DwError> {
        let mut bytes = [0u8; 4];
        for b in &mut bytes {
            *b = self.read_raw_byte()?;
        }
        Ok(u32::from_le_bytes(bytes))
    }

    // --- Write methods ---

    pub fn write_bool(&mut self, val: bool) {
        self.write_type(1);
        self.data.push(if val { 1 } else { 0 });
    }

    pub fn write_ubyte(&mut self, val: u8) {
        self.write_type(3);
        self.data.push(val);
    }

    pub fn write_byte(&mut self, val: i8) {
        self.write_type(2);
        self.data.push(val as u8);
    }

    pub fn write_uint16(&mut self, val: u16) {
        self.write_type(5);
        self.data.extend_from_slice(&val.to_le_bytes());
    }

    pub fn write_int32(&mut self, val: i32) {
        self.write_type(6);
        self.data.extend_from_slice(&val.to_le_bytes());
    }

    pub fn write_uint32(&mut self, val: u32) {
        self.write_type(7);
        self.data.extend_from_slice(&val.to_le_bytes());
    }

    pub fn write_int64(&mut self, val: i64) {
        self.write_type(8);
        self.data.extend_from_slice(&val.to_le_bytes());
    }

    pub fn write_uint64(&mut self, val: u64) {
        self.write_type(9);
        self.data.extend_from_slice(&val.to_le_bytes());
    }

    pub fn write_float(&mut self, val: f32) {
        self.write_type(10);
        self.data.extend_from_slice(&val.to_le_bytes());
    }

    pub fn write_string(&mut self, val: &str) {
        self.write_type(16);
        self.data.extend_from_slice(&(val.len() as u32).to_le_bytes());
        self.data.extend_from_slice(val.as_bytes());
    }

    pub fn write_blob(&mut self, val: &[u8]) {
        self.write_type(13);
        self.data.extend_from_slice(&(val.len() as u32).to_le_bytes());
        self.data.extend_from_slice(val);
    }

    pub fn write_raw(&mut self, data: &[u8]) {
        self.data.extend_from_slice(data);
    }
}

impl Default for ByteBuffer {
    fn default() -> Self {
        Self::new()
    }
}
