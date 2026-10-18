use crate::reader::decode_string;

pub struct Writer {
    buffer: Vec<u8>,
    pos: usize,
}

impl Writer {
    /// Size at fw 6.2
    pub const INSTRUMENT_FILE_SIZE: usize = 357;
    pub const INSTRUMENT_FILE_SIZE_WITH_EQ: usize = 375;

    /// Initialize the writer from a loaded song
    pub fn new(v: Vec<u8>) -> Writer {
        Writer { buffer: v, pos: 0 }
    }

    pub fn new_instrument_writer(with_eq: bool) -> Writer {
        let file_size = if with_eq {
            Writer::INSTRUMENT_FILE_SIZE_WITH_EQ
        } else {
            Writer::INSTRUMENT_FILE_SIZE
        };

        Writer {
            buffer: vec![0; file_size],
            pos: 0,
        }
    }

    /// Terminate writing and return the buffer
    pub fn finish(self) -> Vec<u8> {
        self.buffer
    }

    pub fn write(&mut self, v: u8) {
        self.buffer[self.pos] = v;
        self.pos += 1;
    }

    pub fn write_bytes(&mut self, bytes: &[u8]) {
        let mut cursor = self.pos;
        let buff = &mut self.buffer;

        for b in bytes {
            buff[cursor] = *b;
            cursor += 1;
        }

        self.pos = cursor;
    }

    /// Writes a fixed-size string field of `fill` bytes.
    ///
    /// When the buffer already holds the same text, the field is left untouched,
    /// so the padding after the terminator (`0xFF` fill, stale characters of an
    /// older value) survives a round trip byte for byte.
    pub fn write_string(&mut self, str: &str, fill: usize) {
        if decode_string(&self.buffer[self.pos..self.pos + fill]) == str {
            self.pos += fill;
            return;
        }
        let bytes = str.as_bytes();
        self.write_bytes(bytes);
        self.fill_till(0, fill - bytes.len());
    }

    pub fn skip(&mut self, skip: usize) {
        self.pos += skip
    }

    pub fn seek(&mut self, new_pos: usize) {
        self.pos = new_pos;
    }

    /// Byte at the current position, if any.
    pub fn peek(&self) -> Option<u8> {
        self.buffer.get(self.pos).copied()
    }

    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    fn fill_till(&mut self, v: u8, until: usize) {
        if until == 0 {
            return;
        }

        for _i in 0..until {
            self.buffer[self.pos] = v;
            self.pos += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchanged_string_keeps_its_padding() {
        let mut w = Writer::new(b"KICK\xFF\xFF\x00old".to_vec());
        w.write_string("KICK", 10);
        assert_eq!(w.pos(), 10);
        assert_eq!(w.finish(), b"KICK\xFF\xFF\x00old");
    }

    #[test]
    fn changed_string_is_written_and_zero_filled() {
        let mut w = Writer::new(b"KICK\xFF\xFF\x00old".to_vec());
        w.write_string("SN", 10);
        assert_eq!(w.finish(), b"SN\x00\x00\x00\x00\x00\x00\x00\x00");
    }
}
