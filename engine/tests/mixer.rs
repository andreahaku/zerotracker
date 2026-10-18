//! Optional audible mixer check using the private CALWAV3 calibration song.

use engine::{
    PlaybackSnapshot, TransportMode, ZtEngine, zt_engine_create, zt_engine_destroy,
    zt_engine_get_track_flags, zt_engine_render, zt_engine_set_track_flags,
    zt_engine_track_activity, zt_engine_track_peaks,
};
use m8fmt::{Song, reader::Reader};

fn render(song: Song, mute: u8, solo: u8) -> Vec<i16> {
    let snapshot = PlaybackSnapshot::new(song.clone(), vec![None; song.instruments.len()]).unwrap();
    let ctx = zt_engine_create(44_100, 0);
    assert!(!ctx.is_null());
    let engine: &ZtEngine = unsafe { &*ctx };
    engine
        .publish(Box::new(snapshot))
        .unwrap_or_else(|_| panic!("publish"));
    assert!(engine.request_transport(TransportMode::Song, 0, 0, 0));
    assert_eq!(unsafe { zt_engine_set_track_flags(ctx, mute, solo) }, 1);
    let mut read_mute = 0;
    let mut read_solo = 0;
    assert_eq!(
        unsafe { zt_engine_get_track_flags(ctx, &mut read_mute, &mut read_solo) },
        1
    );
    assert_eq!((read_mute, read_solo), (mute, solo));
    let mut output = vec![0_i16; 8192 * 2];
    for chunk in output.chunks_mut(256 * 2) {
        assert_eq!(
            unsafe { zt_engine_render(ctx, chunk.as_mut_ptr(), 256, 2) },
            1
        );
    }
    unsafe { zt_engine_destroy(ctx) };
    output
}

fn render_with_track_meters(song: Song, mute: u8, solo: u8) -> (Vec<i16>, [u16; 18], [u16; 16]) {
    let snapshot = PlaybackSnapshot::new(song.clone(), vec![None; song.instruments.len()]).unwrap();
    let ctx = zt_engine_create(44_100, 0);
    assert!(!ctx.is_null());
    let engine: &ZtEngine = unsafe { &*ctx };
    engine
        .publish(Box::new(snapshot))
        .unwrap_or_else(|_| panic!("publish"));
    assert!(engine.request_transport(TransportMode::Song, 0, 0, 0));
    assert_eq!(unsafe { zt_engine_set_track_flags(ctx, mute, solo) }, 1);
    let mut output = vec![0_i16; 8192 * 2];
    for chunk in output.chunks_mut(256 * 2) {
        assert_eq!(
            unsafe { zt_engine_render(ctx, chunk.as_mut_ptr(), 256, 2) },
            1
        );
    }
    let mut peaks = [0_u16; 18];
    let mut activity = [0_u16; 16];
    assert_eq!(unsafe { zt_engine_track_peaks(ctx, peaks.as_mut_ptr()) }, 1);
    assert_eq!(
        unsafe { zt_engine_track_activity(ctx, activity.as_mut_ptr()) },
        1
    );
    unsafe { zt_engine_destroy(ctx) };
    (output, peaks, activity)
}

fn rms(samples: &[i16]) -> f64 {
    (samples
        .iter()
        .map(|sample| f64::from(*sample).powi(2))
        .sum::<f64>()
        / samples.len() as f64)
        .sqrt()
}

fn first_sound_rms(samples: &[i16], frames: usize) -> f64 {
    let first = samples
        .as_chunks::<2>()
        .0
        .iter()
        .position(|frame| frame[0] != 0 || frame[1] != 0)
        .expect("audio has an onset");
    rms(&samples[first * 2..(first + frames) * 2])
}

#[test]
fn live_flags_and_saved_mixer_volume_change_audio() {
    let Some(directory) = std::env::var_os("ZT_M8_CALIB_DIR") else {
        return;
    };
    let bytes = std::fs::read(std::path::Path::new(&directory).join("CALWAV3.m8s")).unwrap();
    let song = Song::read_from_reader(&mut Reader::new(bytes)).unwrap();
    assert_eq!(song.mixer_settings.master_volume, 0xE0);
    assert_eq!(song.mixer_settings.track_volume[0], 0xE0);
    let base = render(song.clone(), 0, 0);
    assert!(rms(&base) > 100.0);
    assert_eq!(render(song.clone(), 1, 0), vec![0; base.len()]);
    assert_eq!(render(song.clone(), 0, 2), vec![0; base.len()]);
    assert_eq!(render(song.clone(), 1, 1), vec![0; base.len()]);
    assert_eq!(render(song.clone(), 0, 1), base);

    let (soloed_pcm, post_gate_peaks, pre_gate_activity) =
        render_with_track_meters(song.clone(), 0, 2);
    assert_eq!(soloed_pcm, vec![0; base.len()]);
    assert_eq!(&post_gate_peaks[..2], &[0, 0]);
    assert!(pre_gate_activity[..2].iter().any(|&peak| peak > 0));

    let mut track_quiet = song.clone();
    track_quiet.mixer_settings.track_volume[0] = 0x70;
    let track_ratio = rms(&render(track_quiet, 0, 0)) / rms(&base);
    let expected_track_ratio = 10.0_f64.powf((f64::from(0x70_u8) - 224.0) * 0.2397 / 20.0);
    assert!((track_ratio - expected_track_ratio).abs() < 0.001);
    let mut master_quiet = song;
    master_quiet.mixer_settings.master_volume = 0x70;
    let master_ratio = rms(&render(master_quiet, 0, 0)) / rms(&base);
    assert!((master_ratio - 0.5).abs() < 0.01);
}

#[test]
fn a_wet_only_instrument_reaches_the_chorus_return() {
    let Some(directory) = std::env::var_os("ZT_M8_CALIB_DIR") else {
        return;
    };
    let bytes = std::fs::read(std::path::Path::new(&directory).join("CALWAV3.m8s")).unwrap();
    let mut song = Song::read_from_reader(&mut Reader::new(bytes)).unwrap();
    let m8fmt::Instrument::WavSynth(instrument) = &mut song.instruments[0] else {
        panic!("CALWAV3 must be WAVSYNTH");
    };
    instrument.synth_params.mixer_dry = 0;
    instrument.synth_params.mixer_delay = 0;
    instrument.synth_params.mixer_reverb = 0;
    instrument.synth_params.mixer_mfx = 0x80;
    let wet = render(song.clone(), 0, 0);
    assert!(rms(&wet) > 1.0);
    let m8fmt::Instrument::WavSynth(instrument) = &mut song.instruments[0] else {
        unreachable!();
    };
    instrument.synth_params.mixer_mfx = 0;
    assert_eq!(render(song, 0, 0), vec![0; wet.len()]);
}

#[test]
fn isolated_chorus_sends_match_reference_levels() {
    let Some(directory) = std::env::var_os("ZT_M8_CALIB_DIR") else {
        return;
    };
    let directory = std::path::Path::new(&directory);
    let reference_dir = directory.join("batch8");
    if !reference_dir.join("CALMX80.pcm").exists() || !reference_dir.join("CALMXE0.pcm").exists() {
        return;
    }
    let bytes = std::fs::read(directory.join("CALEDRY.m8s")).unwrap();
    let song = Song::read_from_reader(&mut Reader::new(bytes)).unwrap();
    let mut errors = Vec::new();
    for (name, send) in [("CALMX80", 0x80), ("CALMXE0", 0xe0)] {
        let mut variant = song.clone();
        let m8fmt::Instrument::WavSynth(instrument) = &mut variant.instruments[0] else {
            panic!("CALEDRY must be WAVSYNTH");
        };
        instrument.synth_params.mixer_dry = 0;
        instrument.synth_params.mixer_mfx = send;
        let generated = render(variant, 0, 0);
        let reference_bytes = std::fs::read(reference_dir.join(format!("{name}.pcm"))).unwrap();
        let reference = reference_bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| i16::from_le_bytes(*pair))
            .collect::<Vec<_>>();
        let frames = 4_410;
        let ratio = first_sound_rms(&generated, frames) / first_sound_rms(&reference, frames);
        errors.push(20.0 * ratio.log10());
    }
    assert!(errors.iter().all(|error| error.abs() < 0.6), "{errors:?}");
}
