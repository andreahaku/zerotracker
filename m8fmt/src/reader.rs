use std::fmt;

#[derive(PartialEq, Debug)]
pub struct ParseError(pub String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ParseError: {}", self.0)
    }
}

impl std::error::Error for ParseError {}

/// Spefic result type for M8 song parsing
pub type M8Result<T> = std::result::Result<T, ParseError>;

pub struct Reader {
    buffer: Vec<u8>,
    position: usize,
}

#[allow(dead_code)]
impl Reader {
    pub fn new(buffer: Vec<u8>) -> Self {
        Self {
            buffer,
            position: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    pub fn read(&mut self) -> u8 {
        let p: usize = self.position;
        let b = self.buffer[p];
        self.position += 1;
        b
    }

    pub fn read_bytes(&mut self, n: usize) -> &[u8] {
        let p: usize = self.position;
        let bs = &self.buffer[p..p + n];
        self.position += n;
        bs
    }

    pub fn read_bool(&mut self) -> bool {
        self.read() == 1
    }

    pub fn read_string(&mut self, n: usize) -> String {
        decode_string(self.read_bytes(n))
    }

    pub fn pos(&self) -> usize {
        self.position
    }

    pub fn skip(&mut self, by: usize) {
        self.position += by
    }

    pub fn set_pos(&mut self, n: usize) {
        self.position = n;
    }
}

/// Decodes a fixed-size M8 string field: the text ends at the first `0x00` or
/// `0xFF`, and the bytes after it are padding. Invalid UTF-8 is cut back to the
/// longest valid prefix.
pub fn decode_string(b: &[u8]) -> String {
    let mut end = b
        .iter()
        .position(|&x| x == 0 || x == 255)
        .unwrap_or(b.len());

    while end > 0 {
        match std::str::from_utf8(&b[0..end]) {
            Ok(str) => return str.to_string(),
            Err(_) => end -= 1,
        }
    }

    String::from("")
}
