//! Optional integration checks against Andrea's private song corpus.
//! Set ZT_M8_TEST_SONG to REFERENCE_SONG.m8s to enable them.

use engine::{
    PlaybackSnapshot, Sample, TransportMode, ZtEngine, zt_engine_create, zt_engine_destroy,
    zt_engine_render,
};
use m8fmt::{Song, reader::Reader};
use std::sync::Arc;

fn fixture() -> Option<Song> {
    let path = std::env::var_os("ZT_M8_TEST_SONG")?;
    let bytes = std::fs::read(path).expect("test song is readable");
    Some(Song::read_from_reader(&mut Reader::new(bytes)).expect("song parses"))
}

fn snapshot(song: Song, pcm: Arc<[f32]>) -> PlaybackSnapshot {
    let mut samples = vec![None; song.instruments.len()];
    samples[0] = Some(Sample {
        rate: 44_100,
        channels: 1,
        pcm,
    });
    PlaybackSnapshot::new(song, samples).expect("valid playback data")
}

#[test]
fn phrase_audition_renders_and_retires_snapshots() {
    let Some(mut song) = fixture() else { return };
    assert!(song.phrases[0].steps[0].note.0 < 128);
    assert_eq!(song.phrases[0].steps[0].instrument, 0);
    if let m8fmt::Instrument::Sampler(ref mut instrument) = song.instruments[0] {
        instrument.synth_params.amp = 0;
        instrument.synth_params.mixer_pan = 128;
    } else {
        panic!("instrument 00 must be SAMPLER");
    }
    let pcm: Arc<[f32]> = vec![0.2; 4096].into();
    let ctx = zt_engine_create(44_100, 0);
    assert!(!ctx.is_null());
    // SAFETY: the test owns the engine pointer until rendering ends.
    let engine: &ZtEngine = unsafe { &*ctx };
    engine
        .publish(Box::new(snapshot(song.clone(), pcm.clone())))
        .unwrap_or_else(|_| panic!("first publish"));
    assert!(engine.request_transport(TransportMode::Phrase, 0, 0, 0));
    let mut output = [0_i16; 512];
    assert_eq!(
        unsafe { zt_engine_render(ctx, output.as_mut_ptr(), 256, 2) },
        1
    );
    assert!(output.iter().any(|&sample| sample != 0));
    let status = engine.track_status(0).unwrap();
    assert_eq!(
        (status.phrase, status.phrase_step, status.active),
        (0, 0, 1)
    );
    assert!(status.peak > 0);
    assert!(engine.collect_retired().is_none()); // Initial active was null.
    engine
        .publish(Box::new(snapshot(song, pcm.clone())))
        .unwrap_or_else(|_| panic!("second publish"));
    assert_eq!(
        unsafe { zt_engine_render(ctx, output.as_mut_ptr(), 256, 2) },
        1
    );
    let retired = engine.collect_retired().expect("old snapshot acknowledged");
    drop(retired);
    unsafe { zt_engine_destroy(ctx) };
    assert_eq!(Arc::strong_count(&pcm), 1);
}

#[test]
fn transport_progress_is_independent_of_render_block_size() {
    let Some(song) = fixture() else { return };
    let pcm: Arc<[f32]> = vec![0.1; 4096].into();
    let first = zt_engine_create(44_100, 0);
    let second = zt_engine_create(44_100, 0);
    assert!(!first.is_null() && !second.is_null());
    // SAFETY: both contexts remain live until the test finishes.
    let a = unsafe { &*first };
    let b = unsafe { &*second };
    a.publish(Box::new(snapshot(song.clone(), pcm.clone())))
        .unwrap_or_else(|_| panic!("publish first"));
    b.publish(Box::new(snapshot(song, pcm)))
        .unwrap_or_else(|_| panic!("publish second"));
    a.request_transport(TransportMode::Phrase, 0, 0, 0);
    b.request_transport(TransportMode::Phrase, 0, 0, 0);
    let mut one_block = vec![0_i16; 12_000 * 2];
    let mut many_blocks = vec![0_i16; 12_000 * 2];
    assert_eq!(
        unsafe { zt_engine_render(first, one_block.as_mut_ptr(), 12_000, 2) },
        1
    );
    for block in many_blocks.chunks_mut(256 * 2) {
        assert_eq!(
            unsafe { zt_engine_render(second, block.as_mut_ptr(), block.len() / 2, 2) },
            1
        );
    }
    assert_eq!(one_block, many_blocks);
    let a_status = a.track_status(0).unwrap();
    let b_status = b.track_status(0).unwrap();
    // Peak telemetry describes the most recent render block, whose size
    // differs here; sequencer position must be independent of block size.
    assert_eq!(a_status.song_row, b_status.song_row);
    assert_eq!(a_status.chain_row, b_status.chain_row);
    assert_eq!(a_status.phrase_step, b_status.phrase_step);
    assert_eq!(a_status.active, b_status.active);
    assert!(a_status.phrase_step > 0);
    unsafe { zt_engine_destroy(first) };
    unsafe { zt_engine_destroy(second) };
}

#[test]
fn song_column_loops_at_first_empty_row() {
    let Some(mut song) = fixture() else { return };
    song.song.steps.fill(0xff);
    song.song.steps[0] = 0;
    for step in &mut song.chains[0].steps {
        step.phrase = 0xff;
    }
    song.chains[0].steps[0].phrase = 0;
    let pcm: Arc<[f32]> = vec![0.1; 4096].into();
    let ctx = zt_engine_create(44_100, 0);
    assert!(!ctx.is_null());
    let engine = unsafe { &*ctx };
    engine
        .publish(Box::new(snapshot(song, pcm)))
        .unwrap_or_else(|_| panic!("publish"));
    engine.request_transport(TransportMode::Song, 0, 0, 0);
    let mut block = [0_i16; 512];
    for _ in 0..(44_100 * 6 / 256) {
        assert_eq!(
            unsafe { zt_engine_render(ctx, block.as_mut_ptr(), 256, 2) },
            1
        );
    }
    let status = engine.track_status(0).unwrap();
    assert_eq!(status.song_row, 0);
    assert_eq!(status.active, 1);
    unsafe { zt_engine_destroy(ctx) };
}

#[test]
fn first_empty_chain_row_advances_to_next_song_row() {
    let Some(mut song) = fixture() else { return };
    song.tempo = 120.0;
    song.song.steps.fill(0xff);
    song.song.steps[0] = 0;
    song.song.steps[8] = 1;
    for chain in &mut song.chains[..2] {
        for step in &mut chain.steps {
            step.phrase = 0xff;
        }
    }
    song.chains[0].steps[0].phrase = 0;
    song.chains[0].steps[2].phrase = 0; // Must not play after the hole.
    song.chains[1].steps[0].phrase = 0;
    song.grooves[0].steps.fill(0xff);
    song.grooves[0].steps[0] = 6;
    let ctx = zt_engine_create(44_100, 0);
    assert!(!ctx.is_null());
    let engine = unsafe { &*ctx };
    engine
        .publish(Box::new(snapshot(song, vec![0.1; 4096].into())))
        .unwrap_or_else(|_| panic!("publish"));
    engine.request_transport(TransportMode::Song, 0, 0, 0);
    let mut block = [0_i16; 512];
    // One 16-step phrase at 120 BPM lasts two seconds.
    for _ in 0..(44_100 * 3 / 256) {
        assert_eq!(
            unsafe { zt_engine_render(ctx, block.as_mut_ptr(), 256, 2) },
            1
        );
    }
    let status = engine.track_status(0).unwrap();
    assert_eq!(status.song_row, 1);
    assert_eq!(status.chain_row, 0);
    unsafe { zt_engine_destroy(ctx) };
}

#[test]
fn hop_jumps_immediately_without_consuming_its_step_duration() {
    let Some(mut song) = fixture() else { return };
    song.tempo = 120.0;
    song.song.steps.fill(0xff);
    song.song.steps[0] = 0;
    song.song.steps[8] = 1;
    for chain in &mut song.chains[..2] {
        for step in &mut chain.steps {
            step.phrase = 0xff;
        }
    }
    song.chains[0].steps[0].phrase = 0;
    song.chains[1].steps[0].phrase = 1;
    song.grooves[0].steps.fill(0xff);
    song.grooves[0].steps[0] = 6;
    for step in &mut song.phrases[0].steps[..3] {
        step.note.0 = 0xff;
        step.instrument = 0xff;
        step.velocity = 0xff;
        step.fx1 = m8fmt::FX::default();
        step.fx2 = m8fmt::FX::default();
        step.fx3 = m8fmt::FX::default();
    }
    let hop = m8fmt::FX::fx_command_names(song.version).find_indices(&["HOP"])[0];
    song.phrases[0].steps[2].fx1 = m8fmt::FX {
        command: hop,
        value: 0,
    };

    let ctx = zt_engine_create(8_000, 0);
    let engine = unsafe { &*ctx };
    engine
        .publish(Box::new(snapshot(song, vec![0.1; 4096].into())))
        .unwrap_or_else(|_| panic!("publish"));
    engine.request_transport(TransportMode::Song, 0, 0, 0);
    let mut block = [0_i16; 512];
    let mut row_one_time = None;
    for index in 0..20 {
        assert_eq!(
            unsafe { zt_engine_render(ctx, block.as_mut_ptr(), 256, 2) },
            1
        );
        if engine.track_status(0).unwrap().song_row == 1 {
            row_one_time = Some((index + 1) as f64 * 256.0 / 8_000.0);
            break;
        }
    }
    assert!(row_one_time.is_some_and(|seconds| (0.24..0.30).contains(&seconds)));
    unsafe { zt_engine_destroy(ctx) };
}
