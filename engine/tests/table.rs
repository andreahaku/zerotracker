//! Optional comparisons of controlled table songs from the private reference set.
//! Set ZT_M8_CALIB_DIR to the directory containing their .m8s and .pcm files.

use engine::{
    PlaybackSnapshot, TransportMode, ZtEngine, zt_engine_create, zt_engine_destroy,
    zt_engine_render,
};
use m8fmt::{Song, reader::Reader};
use std::path::Path;

const SAMPLE_RATE: usize = 44_100;

fn render(path: &Path, block_frames: usize) -> Vec<i16> {
    let bytes = std::fs::read(path).expect("calibration song is readable");
    let song = Song::read_from_reader(&mut Reader::new(bytes)).expect("calibration song parses");
    let slots = vec![None; song.instruments.len()];
    let snapshot = PlaybackSnapshot::new(song, slots).expect("calibration snapshot");
    let ctx = zt_engine_create(SAMPLE_RATE as u32, 0);
    assert!(!ctx.is_null());
    // SAFETY: this test owns and exclusively renders the engine until destroy.
    let engine: &ZtEngine = unsafe { &*ctx };
    engine
        .publish(Box::new(snapshot))
        .unwrap_or_else(|_| panic!("publish"));
    assert!(engine.request_transport(TransportMode::Song, 0, 0, 0));
    let mut output = vec![0_i16; SAMPLE_RATE * 2 * 2];
    for chunk in output.chunks_mut(block_frames * 2) {
        assert_eq!(
            unsafe { zt_engine_render(ctx, chunk.as_mut_ptr(), chunk.len() / 2, 2) },
            1
        );
    }
    unsafe { zt_engine_destroy(ctx) };
    output
}

fn frequency_in_row(samples: &[i16], origin: usize, row: usize) -> f64 {
    let start = origin + ((row as f64 * 0.125 + 0.01) * SAMPLE_RATE as f64) as usize;
    let end = origin + (((row + 1) as f64 * 0.125 - 0.01) * SAMPLE_RATE as f64) as usize;
    let mono = samples[start * 2..end * 2]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|frame| frame[0])
        .collect::<Vec<_>>();
    let edges = mono
        .windows(2)
        .enumerate()
        .filter_map(|(index, pair)| (pair[0] < 0 && pair[1] >= 0).then_some(index))
        .collect::<Vec<_>>();
    let mut periods = edges
        .windows(2)
        .map(|pair| pair[1] - pair[0])
        .collect::<Vec<_>>();
    periods.sort_unstable();
    SAMPLE_RATE as f64 / periods[periods.len() / 2] as f64
}

fn reference_capture(directory: &Path, name: &str) -> (Vec<i16>, usize) {
    let path = directory.join(format!("{name}.pcm"));
    let bytes = std::fs::read(path).expect("reference capture is readable");
    let samples = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    let origin = samples
        .as_chunks::<2>()
        .0
        .iter()
        .position(|frame| frame[0].unsigned_abs() > 100)
        .expect("reference capture contains audio");
    (samples, origin)
}

fn row_rms(samples: &[i16], origin: usize, row: usize) -> f64 {
    let start = origin + ((row as f64 * 0.125 + 0.025) * SAMPLE_RATE as f64) as usize;
    let end = origin + (((row + 1) as f64 * 0.125 - 0.025) * SAMPLE_RATE as f64) as usize;
    let frames = samples[start * 2..end * 2].as_chunks::<2>().0;
    let power = frames
        .iter()
        .map(|frame| f64::from(frame[0]).powi(2))
        .sum::<f64>();
    (power / frames.len() as f64).sqrt()
}

#[test]
fn dedicated_and_fx_selected_tables_render_the_same_arpeggio() {
    let Some(directory) = std::env::var_os("ZT_M8_CALIB_DIR") else {
        return;
    };
    let directory = Path::new(&directory);
    let dedicated = render(&directory.join("CALTBL.m8s"), 256);
    let selected = render(&directory.join("CALFX.m8s"), 127);
    assert_eq!(dedicated, selected);
    for (row, expected) in [(0, 65.4), (1, 82.4), (2, 98.0), (3, 130.8)] {
        assert!((frequency_in_row(&dedicated, 0, row) - expected).abs() < 1.0);
    }
}

#[test]
fn controlled_tables_match_optional_reference_pitch_and_row_timing() {
    let Some(directory) = std::env::var_os("ZT_M8_CALIB_DIR") else {
        return;
    };
    for name in ["CALTBL", "CALFX"] {
        let (samples, origin) = reference_capture(Path::new(&directory), name);
        for (row, expected) in [(0, 65.4), (1, 82.4), (2, 98.0), (3, 130.8)] {
            assert!(
                (frequency_in_row(&samples, origin, row) - expected).abs() < 1.0,
                "{name} row {row} does not match the 125 ms table schedule"
            );
        }
    }
}

#[test]
fn table_velocity_and_kill_follow_optional_reference_controls() {
    let Some(directory) = std::env::var_os("ZT_M8_CALIB_DIR") else {
        return;
    };
    let directory = Path::new(&directory);
    let (velocity_reference, velocity_origin) = reference_capture(directory, "CALTVEL");
    let velocity_engine = render(&directory.join("CALTVEL.m8s"), 256);
    let reference_base = row_rms(&velocity_reference, velocity_origin, 0);
    let engine_base = row_rms(&velocity_engine, 0, 0);
    for row in 1..=4 {
        let reference_ratio = row_rms(&velocity_reference, velocity_origin, row) / reference_base;
        let engine_ratio = row_rms(&velocity_engine, 0, row) / engine_base;
        assert!(
            (engine_ratio / reference_ratio - 1.0).abs() < 0.1,
            "table velocity ratio at row {row} differs from reference"
        );
    }

    let (kill_reference, kill_origin) = reference_capture(directory, "CALTKIL");
    let kill_engine = render(&directory.join("CALTKIL.m8s"), 127);
    for (samples, origin) in [(&kill_reference, kill_origin), (&kill_engine, 0)] {
        assert!(row_rms(samples, origin, 1) > 1000.0);
        assert!(row_rms(samples, origin, 3) < 1.0);
        assert!(row_rms(samples, origin, 8) > 100.0);
    }
}
