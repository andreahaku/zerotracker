use crate::reader::*;
use crate::version::*;
use crate::writer::Writer;

use std::fmt;

use arr_macro::arr;
use byteorder::{ByteOrder, LittleEndian};

#[derive(PartialEq, Clone)]
pub struct Scale {
    pub number: u8,
    pub name: String,
    pub notes: [NoteOffset; 12], // Offsets for notes C-B
    /// Top 4 bits of the 16-bit note-enable map, not used by the 12 notes.
    /// Kept verbatim (the corpus has 0xF for many scales).
    pub map_high_bits: u8,
}

impl Scale {
    const SIZE: usize = 32;

    pub fn read(reader: &mut impl std::io::Read) -> M8Result<Self> {
        let mut buf: Vec<u8> = vec![];
        reader.read_to_end(&mut buf).unwrap();
        let len = buf.len();
        let mut reader = Reader::new(buf);

        if len < Self::SIZE + Version::SIZE {
            return Err(ParseError(
                "File is not long enough to be a M8 Scale".to_string(),
            ));
        }
        Version::from_reader(&mut reader)?;
        Self::from_reader(&mut reader, 0)
    }

    /// Mirror of `from_reader`: enable bitmap, the two raw offset bytes of
    /// each note, then the name.
    pub fn write(&self, w: &mut Writer) {
        let mut map: u16 = u16::from(self.map_high_bits) << 12;
        for (i, note) in self.notes.iter().enumerate() {
            if note.enabled {
                map |= 1 << i;
            }
        }
        let mut buf = [0u8; 2];
        LittleEndian::write_u16(&mut buf, map);
        w.write_bytes(&buf);
        for note in &self.notes {
            w.write(note.semitone_byte);
            w.write(note.cents_byte);
        }
        w.write_string(&self.name, 16);
    }

    pub(crate) fn from_reader(reader: &mut Reader, number: u8) -> M8Result<Self> {
        let map = LittleEndian::read_u16(reader.read_bytes(2));
        let mut notes = arr![NoteOffset::default(); 12];

        for (i, note) in notes.iter_mut().enumerate() {
            note.enabled = ((map >> i) & 0x1) == 1;
            note.semitone_byte = reader.read();
            note.cents_byte = reader.read();
        }

        let name = reader.read_string(16);
        Ok(Self {
            number,
            name,
            notes,
            map_high_bits: (map >> 12) as u8,
        })
    }
}

impl Default for Scale {
    fn default() -> Self {
        Self {
            number: 0,
            name: "CHROMATIC".to_string(),
            notes: arr![NoteOffset::default(); 12],
            map_high_bits: 0,
        }
    }
}

impl fmt::Display for Scale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let notes = vec![
            "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
        ];
        let offsets = self
            .notes
            .iter()
            .zip(notes.iter())
            .map(|(offset, note)| -> String {
                let s = if offset.enabled {
                    let semitones = offset.semitones();
                    let sign = if semitones < 0.0 { "-" } else { " " };
                    format!(" ON{}{:02.2}", sign, semitones.abs())
                } else {
                    " -- -- --".to_string()
                };
                format!("{:<2}{}", note, s)
            })
            .collect::<Vec<String>>()
            .join("\n");

        write!(
            f,
            "Scale {}\nKEY   C\n\n   EN OFFSET\n{}\n\nNAME  {}",
            self.number, offsets, self.name
        )
    }
}
impl fmt::Debug for Scale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self)
    }
}

#[derive(PartialEq, Debug, Clone, Copy)]
pub struct NoteOffset {
    pub enabled: bool,
    /// Raw offset bytes as stored in the file. Their exact encoding (sign,
    /// cents range, 0xFF) is not confirmed yet, so they are kept verbatim.
    pub semitone_byte: u8,
    pub cents_byte: u8,
}

impl NoteOffset {
    fn default() -> Self {
        Self {
            enabled: true,
            semitone_byte: 0,
            cents_byte: 0,
        }
    }

    /// Offset in semitones, with the upstream m8-file-parser interpretation
    /// (unsigned semitones plus cents / 100). Semitones.cents: -24.0-24.0 in the UI.
    pub fn semitones(&self) -> f32 {
        f32::from(self.semitone_byte) + f32::from(self.cents_byte) / 100.0
    }
}
