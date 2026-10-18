//! Optional phrase OFF checks using a controlled private WAVSYNTH song.

use engine::{
    PlaybackSnapshot, TransportMode, ZtEngine, zt_engine_create, zt_engine_destroy,
    zt_engine_render,
};
use m8fmt::{Song, reader::Reader};

const RATE: usize = 44_100;

fn render(song: Song) -> Vec<i16> {
    let samples = vec![None; song.instruments.len()];
    let snapshot = PlaybackSnapshot::new(song, samples).expect("valid snapshot");
    let ctx = zt_engine_create(RATE as u32, 0);
    assert!(!ctx.is_null());
    // SAFETY: the test owns the engine until rendering ends.
    let engine: &ZtEngine = unsafe { &*ctx };
    assert!(engine.publish(Box::new(snapshot)).is_ok());
    assert!(engine.request_transport(TransportMode::Phrase, 0, 0, 0));
    let mut output = vec![0_i16; RATE * 2];
    for block in output.chunks_mut(256 * 2) {
        assert_eq!(
            unsafe { zt_engine_render(ctx, block.as_mut_ptr(), block.len() / 2, 2) },
            1
        );
    }
    unsafe { zt_engine_destroy(ctx) };
    output
}

fn peak(audio: &[i16], from_ms: usize, to_ms: usize) -> u16 {
    audio[from_ms * RATE * 2 / 1000..to_ms * RATE * 2 / 1000]
        .iter()
        .map(|value| value.unsigned_abs())
        .max()
        .unwrap_or(0)
}

#[test]
fn phrase_off_command_stops_a_voice_and_honors_tick_delay() {
    let Some(directory) = std::env::var_os("ZT_M8_CALIB_DIR") else {
        return;
    };
    let bytes = std::fs::read(std::path::Path::new(&directory).join("CALWAV3.m8s"))
        .expect("CALWAV3 song is readable");
    let mut song = Song::read_from_reader(&mut Reader::new(bytes)).expect("song parses");
    let off = m8fmt::FX::fx_command_names(song.version).find_indices(&["OFF"])[0];
    song.phrases[0].steps[8].note.0 = 0xff;
    song.phrases[0].steps[8].instrument = 0xff;
    song.phrases[0].steps[8].velocity = 0xff;
    let baseline = render(song.clone());
    assert!(peak(&baseline, 170, 190) > 20);

    song.phrases[0].steps[1].fx1 = m8fmt::FX {
        command: off,
        value: 0,
    };
    let immediate = render(song.clone());
    assert!(peak(&immediate, 40, 60) > 20);
    assert_eq!(peak(&immediate, 170, 190), 0);

    song.phrases[0].steps[1].fx1.value = 4;
    let delayed = render(song);
    assert!(peak(&delayed, 150, 170) > 20);
    assert_eq!(peak(&delayed, 250, 270), 0);
}
