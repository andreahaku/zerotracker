//! Atomic transport requests and track telemetry.

use std::sync::atomic::{AtomicU64, Ordering};

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportMode {
    Stop = 0,
    Song = 1,
    Chain = 2,
    Phrase = 3,
}

impl TransportMode {
    pub(crate) fn from_i32(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Stop),
            1 => Some(Self::Song),
            2 => Some(Self::Chain),
            3 => Some(Self::Phrase),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TransportRequest {
    pub mode: TransportMode,
    pub index: u8,
    pub row: u8,
    pub track: u8,
}

fn encode_request(request: TransportRequest, sequence: u32) -> u64 {
    (u64::from(sequence) << 32)
        | (request.mode as u64)
        | (u64::from(request.index) << 8)
        | (u64::from(request.row) << 16)
        | (u64::from(request.track) << 24)
}

fn decode_request(value: u64) -> Option<(u32, TransportRequest)> {
    Some((
        (value >> 32) as u32,
        TransportRequest {
            mode: TransportMode::from_i32((value & 0xff) as i32)?,
            index: (value >> 8) as u8,
            row: (value >> 16) as u8,
            track: (value >> 24) as u8,
        },
    ))
}

pub(crate) struct Transport {
    latest: AtomicU64,
}

impl Transport {
    pub(crate) fn new() -> Self {
        Self {
            latest: AtomicU64::new(encode_request(
                TransportRequest {
                    mode: TransportMode::Stop,
                    index: 0,
                    row: 0,
                    track: 0,
                },
                0,
            )),
        }
    }

    /// Latest request wins if the UI sends several before the next callback.
    pub(crate) fn request(&self, request: TransportRequest) {
        let mut old = self.latest.load(Ordering::Acquire);
        loop {
            let sequence = ((old >> 32) as u32).wrapping_add(1);
            let next = encode_request(request, sequence);
            match self
                .latest
                .compare_exchange_weak(old, next, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => return,
                Err(actual) => old = actual,
            }
        }
    }

    pub(crate) fn take_latest(&self, last_sequence: &mut u32) -> Option<TransportRequest> {
        let (sequence, request) = decode_request(self.latest.load(Ordering::Acquire))?;
        if sequence == *last_sequence {
            return None;
        }
        *last_sequence = sequence;
        Some(request)
    }
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ZtTrackStatus {
    pub song_row: u8,
    pub chain: u8,
    pub chain_row: u8,
    pub phrase: u8,
    pub phrase_step: u8,
    pub peak: u16,
    pub active: u8,
}

fn encode_status(status: ZtTrackStatus) -> u64 {
    u64::from(status.song_row)
        | (u64::from(status.chain) << 8)
        | (u64::from(status.chain_row) << 16)
        | (u64::from(status.phrase) << 24)
        | (u64::from(status.phrase_step) << 32)
        | (u64::from(status.peak) << 40)
        | (u64::from(status.active) << 56)
}

fn decode_status(value: u64) -> ZtTrackStatus {
    ZtTrackStatus {
        song_row: value as u8,
        chain: (value >> 8) as u8,
        chain_row: (value >> 16) as u8,
        phrase: (value >> 24) as u8,
        phrase_step: (value >> 32) as u8,
        peak: (value >> 40) as u16,
        active: (value >> 56) as u8,
    }
}

pub(crate) struct Telemetry {
    tracks: [AtomicU64; 8],
}

impl Telemetry {
    pub(crate) fn new() -> Self {
        Self {
            tracks: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }

    pub(crate) fn publish(&self, track: usize, status: ZtTrackStatus) {
        self.tracks[track].store(encode_status(status), Ordering::Release);
    }

    pub(crate) fn read(&self, track: u8) -> Option<ZtTrackStatus> {
        self.tracks
            .get(usize::from(track))
            .map(|status| decode_status(status.load(Ordering::Acquire)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_and_status_round_trip() {
        let transport = Transport::new();
        let mut sequence = 0;
        assert_eq!(transport.take_latest(&mut sequence), None);
        let request = TransportRequest {
            mode: TransportMode::Phrase,
            index: 42,
            row: 13,
            track: 7,
        };
        transport.request(request);
        assert_eq!(transport.take_latest(&mut sequence), Some(request));
        assert_eq!(transport.take_latest(&mut sequence), None);
        let telemetry = Telemetry::new();
        let status = ZtTrackStatus {
            song_row: 25,
            chain: 8,
            chain_row: 15,
            phrase: 42,
            phrase_step: 13,
            peak: 65_535,
            active: 1,
        };
        telemetry.publish(7, status);
        assert_eq!(telemetry.read(7), Some(status));
        assert_eq!(telemetry.read(8), None);
    }
}
