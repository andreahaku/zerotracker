//! Read, edit, write, and read again against upstream fixture files.
//! Run `scripts/fetch-fixtures.sh` first to populate the ignored examples directory.

use m8fmt::reader::Reader;
use m8fmt::writer::Writer;
use m8fmt::{
    AnalogInputSettings, Chain, Instrument, Phrase, Song, Step, Table, TableStep, V4_1_OFFSETS,
    V4_OFFSETS,
};
use std::ops::Range;
use std::path::Path;

const V6_FIXTURE: &str = "CommandMappingV4/CMDMAPPING_6_5.m8s";

fn fixture(name: &str) -> Option<Vec<u8>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples/songs")
        .join(name);
    match std::fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("skipping {}: run scripts/fetch-fixtures.sh", path.display());
            None
        }
        Err(err) => panic!("cannot read {}: {err}", path.display()),
    }
}

fn parse(bytes: &[u8]) -> Song {
    Song::read_from_reader(&mut Reader::new(bytes.to_vec())).expect("fixture parses")
}

fn write(song: &Song, original: &[u8]) -> Vec<u8> {
    let mut writer = Writer::new(original.to_vec());
    song.write(&mut writer).expect("song writes");
    writer.finish()
}

fn assert_only_changed_in(before: &[u8], after: &[u8], allowed: Range<usize>) {
    assert_eq!(before.len(), after.len());
    let changed: Vec<_> = before
        .iter()
        .zip(after)
        .enumerate()
        .filter_map(|(i, (a, b))| (a != b).then_some(i))
        .collect();
    assert!(!changed.is_empty(), "edit changed no bytes");
    assert!(
        changed.iter().all(|i| allowed.contains(i)),
        "bytes changed outside {allowed:?}: {changed:?}"
    );
}

fn checked_edit(
    bytes: &[u8],
    allowed: Range<usize>,
    edit: impl FnOnce(&mut Song),
    check: impl FnOnce(&Song),
) {
    let mut song = parse(bytes);
    assert_eq!(
        write(&song, bytes),
        bytes,
        "unmodified fixture must round-trip"
    );
    edit(&mut song);
    let output = write(&song, bytes);
    assert_only_changed_in(bytes, &output, allowed);
    check(&parse(&output));
}

fn changed(value: u8) -> u8 {
    value ^ 1
}

#[test]
fn pattern_sections_round_trip_with_localized_edits() {
    let Some(bytes) = fixture(V6_FIXTURE) else {
        return;
    };
    let initial = parse(&bytes);
    let expected = changed(initial.song.steps[0]);
    checked_edit(
        &bytes,
        V4_OFFSETS.song..V4_OFFSETS.song + 1,
        |song| song.song.steps[0] = expected,
        |song| assert_eq!(song.song.steps[0], expected),
    );

    let expected = changed(initial.chains[0].steps[0].transpose);
    let offset = V4_OFFSETS.chains + 1;
    checked_edit(
        &bytes,
        offset..offset + 1,
        |song| song.chains[0].steps[0].transpose = expected,
        |song| assert_eq!(song.chains[0].steps[0].transpose, expected),
    );

    let expected = changed(initial.phrases[0].steps[0].velocity);
    let offset = V4_OFFSETS.phrases + 1;
    checked_edit(
        &bytes,
        offset..offset + 1,
        |song| song.phrases[0].steps[0].velocity = expected,
        |song| assert_eq!(song.phrases[0].steps[0].velocity, expected),
    );

    let expected = changed(initial.tables[0].steps[0].velocity);
    let offset = V4_OFFSETS.table + 1;
    checked_edit(
        &bytes,
        offset..offset + 1,
        |song| song.tables[0].steps[0].velocity = expected,
        |song| assert_eq!(song.tables[0].steps[0].velocity, expected),
    );

    assert_eq!(Chain::V4_SIZE, 32);
    assert_eq!(Phrase::V4_SIZE, 16 * Step::V4_SIZE);
    assert_eq!(Table::V4_SIZE, 16 * TableStep::V4_SIZE);
}

#[test]
fn phrase_fx_commands_and_values_round_trip() {
    let Some(bytes) = fixture(V6_FIXTURE) else {
        return;
    };
    let original = parse(&bytes);
    for lane in 0..3 {
        let prior = &original.phrases[0].steps[0];
        let fx = [prior.fx1, prior.fx2, prior.fx3][lane];
        let command = changed(fx.command);
        let value = changed(fx.value);
        let offset = V4_OFFSETS.phrases + 3 + lane * 2;
        checked_edit(
            &bytes,
            offset..offset + 2,
            |song| {
                let step = &mut song.phrases[0].steps[0];
                let fx = match lane {
                    0 => &mut step.fx1,
                    1 => &mut step.fx2,
                    _ => &mut step.fx3,
                };
                fx.command = command;
                fx.value = value;
            },
            |song| {
                let step = &song.phrases[0].steps[0];
                let fx = [step.fx1, step.fx2, step.fx3][lane];
                assert_eq!((fx.command, fx.value), (command, value));
            },
        );
    }
}

#[test]
fn each_instrument_type_round_trips_one_parameter() {
    let Some(bytes) = fixture(V6_FIXTURE) else {
        return;
    };
    let initial = parse(&bytes);
    for kind in 0..=6 {
        let index = initial
            .instruments
            .iter()
            .position(|instrument| {
                matches!(
                    (kind, instrument),
                    (0, Instrument::WavSynth(_))
                        | (1, Instrument::MacroSynth(_))
                        | (2, Instrument::Sampler(_))
                        | (3, Instrument::MIDIOut(_))
                        | (4, Instrument::FMSynth(_))
                        | (5, Instrument::HyperSynth(_))
                        | (6, Instrument::External(_))
                )
            })
            .expect("fixture contains every instrument type");
        let base = V4_OFFSETS.instruments + index * Instrument::INSTRUMENT_MEMORY_SIZE;
        let (field_offset, before) = match &initial.instruments[index] {
            Instrument::WavSynth(v) => (19, v.size),
            Instrument::MacroSynth(v) => (19, v.timbre),
            Instrument::Sampler(v) => (19, v.slice),
            Instrument::MIDIOut(v) => (16, v.channel),
            Instrument::FMSynth(v) => (18, v.algo.0),
            Instrument::HyperSynth(v) => (25, v.scale),
            Instrument::External(v) => (20, v.channel),
            Instrument::None => unreachable!(),
        };
        let expected = changed(before);
        checked_edit(
            &bytes,
            base + field_offset..base + field_offset + 1,
            |song| match &mut song.instruments[index] {
                Instrument::WavSynth(v) => v.size = expected,
                Instrument::MacroSynth(v) => v.timbre = expected,
                Instrument::Sampler(v) => v.slice = expected,
                Instrument::MIDIOut(v) => v.channel = expected,
                Instrument::FMSynth(v) => v.algo.0 = expected,
                Instrument::HyperSynth(v) => v.scale = expected,
                Instrument::External(v) => v.channel = expected,
                Instrument::None => unreachable!(),
            },
            |song| {
                let actual = match &song.instruments[index] {
                    Instrument::WavSynth(v) => v.size,
                    Instrument::MacroSynth(v) => v.timbre,
                    Instrument::Sampler(v) => v.slice,
                    Instrument::MIDIOut(v) => v.channel,
                    Instrument::FMSynth(v) => v.algo.0,
                    Instrument::HyperSynth(v) => v.scale,
                    Instrument::External(v) => v.channel,
                    Instrument::None => unreachable!(),
                };
                assert_eq!(actual, expected, "instrument type {kind}");
            },
        );
    }
}

#[test]
fn shorter_instrument_strings_have_a_terminator() {
    let Some(bytes) = fixture(V6_FIXTURE) else {
        return;
    };
    let initial = parse(&bytes);
    let index = initial
        .instruments
        .iter()
        .position(|v| matches!(v, Instrument::Sampler(_)))
        .expect("fixture contains a sampler");
    let base = V4_OFFSETS.instruments + index * Instrument::INSTRUMENT_MEMORY_SIZE;
    checked_edit(
        &bytes,
        base + 1..base + 13,
        |song| {
            if let Instrument::Sampler(v) = &mut song.instruments[index] {
                v.name = "ZT".into();
            }
        },
        |song| {
            assert!(matches!(&song.instruments[index], Instrument::Sampler(v) if v.name == "ZT"))
        },
    );
    let mut song = parse(&bytes);
    if let Instrument::Sampler(v) = &mut song.instruments[index] {
        v.name = "ZT".into();
    }
    let output = write(&song, &bytes);
    assert_eq!(output[base + 3], 0);

    checked_edit(
        &bytes,
        base + 87..base + 215,
        |song| {
            if let Instrument::Sampler(v) = &mut song.instruments[index] {
                v.sample_path = "/ZT.WAV".into();
            }
        },
        |song| {
            assert!(
                matches!(&song.instruments[index], Instrument::Sampler(v) if v.sample_path == "/ZT.WAV")
            )
        },
    );
    let mut song = parse(&bytes);
    if let Instrument::Sampler(v) = &mut song.instruments[index] {
        v.sample_path = "/ZT.WAV".into();
    }
    let output = write(&song, &bytes);
    assert_eq!(output[base + 87 + "/ZT.WAV".len()], 0);
}

#[test]
fn v4_eq_uses_the_v4_offset() {
    let Some(bytes) = fixture("V4EMPTY.m8s") else {
        return;
    };
    let original = parse(&bytes);
    assert_eq!(original.version.major, 4);
    let expected = changed(original.eqs[0].low.level);
    let offset = V4_OFFSETS.eq + 4;
    checked_edit(
        &bytes,
        offset..offset + 1,
        |song| song.eqs[0].low.level = expected,
        |song| assert_eq!(song.eqs[0].low.level, expected),
    );
}

#[test]
fn optional_private_corpus_song_step_edits() {
    let Some(root) = std::env::var_os("ZT_M8_CORPUS_DIR") else {
        return;
    };
    let mut pending = vec![std::path::PathBuf::from(root)];
    let mut checked = 0;
    let mut unparseable = 0;
    while let Some(path) = pending.pop() {
        for entry in std::fs::read_dir(&path).expect("corpus directory is readable") {
            let entry = entry.expect("corpus entry is readable");
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            if path.extension().is_none_or(|ext| ext != "m8s") {
                continue;
            }
            let bytes = std::fs::read(&path).expect("song is readable");
            let Ok(song) = Song::read_from_reader(&mut Reader::new(bytes.clone())) else {
                unparseable += 1;
                continue;
            };
            if song.version.major != 6 {
                continue;
            }
            let expected = changed(song.song.steps[0]);
            checked_edit(
                &bytes,
                V4_OFFSETS.song..V4_OFFSETS.song + 1,
                |song| song.song.steps[0] = expected,
                |song| assert_eq!(song.song.steps[0], expected),
            );
            checked += 1;
        }
    }
    eprintln!("corpus: {checked} edited 6.x songs, {unparseable} unparseable songs skipped");
    assert!(checked > 0, "corpus has no 6.x songs");
}

#[test]
fn standalone_instrument_with_eq_round_trips_an_edit() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/instruments/FMDUBSTABEQ_4_1.m8i");
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!("skipping {}: run scripts/fetch-fixtures.sh", path.display());
        return;
    };
    let mut instrument = Instrument::read(&mut bytes.as_slice()).expect("instrument parses");
    let mut writer = Writer::new(bytes.clone());
    instrument.write(&mut writer);
    assert_eq!(
        writer.finish(),
        bytes,
        "unmodified instrument must round-trip"
    );
    let expected = changed(instrument.eq.as_ref().expect("fixture has EQ").low.level);
    instrument.eq.as_mut().expect("fixture has EQ").low.level = expected;
    let mut writer = Writer::new(bytes.clone());
    instrument.write(&mut writer);
    let output = writer.finish();
    let eq_offset = 0x165;
    assert_only_changed_in(&bytes, &output, eq_offset + 4..eq_offset + 5);
    let reloaded = Instrument::read(&mut output.as_slice()).expect("edited instrument parses");
    assert_eq!(
        reloaded.eq.expect("edited instrument has EQ").low.level,
        expected
    );
}

#[test]
fn header_and_midi_settings_round_trip() {
    let Some(bytes) = fixture(V6_FIXTURE) else {
        return;
    };
    let initial = parse(&bytes);
    let transpose = changed(initial.transpose);
    checked_edit(
        &bytes,
        142..143,
        |song| song.transpose = transpose,
        |song| assert_eq!(song.transpose, transpose),
    );
    let tempo = initial.tempo + 1.0;
    checked_edit(
        &bytes,
        143..147,
        |song| song.tempo = tempo,
        |song| assert_eq!(song.tempo, tempo),
    );
    let quantize = changed(initial.quantize);
    checked_edit(
        &bytes,
        147..148,
        |song| song.quantize = quantize,
        |song| assert_eq!(song.quantize, quantize),
    );
    let key = changed(initial.key);
    checked_edit(
        &bytes,
        187..188,
        |song| song.key = key,
        |song| assert_eq!(song.key, key),
    );
    let track_channel = changed(initial.midi_settings.track_input_channel[0]);
    checked_edit(
        &bytes,
        169..170,
        |song| song.midi_settings.track_input_channel[0] = track_channel,
        |song| assert_eq!(song.midi_settings.track_input_channel[0], track_channel),
    );
    let mut song = parse(&bytes);
    song.name = "ZT".into();
    let output = write(&song, &bytes);
    assert_only_changed_in(&bytes, &output, 148..160);
    assert_eq!(output[150], 0);
    assert_eq!(parse(&output).name, "ZT");
    song = parse(&bytes);
    song.directory = "ZT".into();
    let output = write(&song, &bytes);
    assert_only_changed_in(&bytes, &output, 14..142);
    assert_eq!(output[16], 0);
    assert_eq!(parse(&output).directory, "ZT");
}

#[test]
fn stereo_and_dual_mono_mixer_round_trip() {
    let Some(stereo_bytes) = fixture(V6_FIXTURE) else {
        return;
    };
    let stereo = parse(&stereo_bytes);
    let AnalogInputSettings::Stereo(left) = &stereo.mixer_settings.analog_input else {
        panic!("fixture must use stereo input");
    };
    let expected = changed(left.mfx);
    checked_edit(
        &stereo_bytes,
        222..223,
        |song| {
            if let AnalogInputSettings::Stereo(left) = &mut song.mixer_settings.analog_input {
                left.mfx = expected;
            }
        },
        |song| {
            assert!(
                matches!(&song.mixer_settings.analog_input, AnalogInputSettings::Stereo(left) if left.mfx == expected)
            )
        },
    );
    let volume = changed(stereo.mixer_settings.master_volume);
    checked_edit(
        &stereo_bytes,
        206..207,
        |song| song.mixer_settings.master_volume = volume,
        |song| assert_eq!(song.mixer_settings.master_volume, volume),
    );

    let Some(dual_bytes) = fixture("CommandMappingV4/CMDMAPPING_6_2.m8s") else {
        return;
    };
    let dual = parse(&dual_bytes);
    let AnalogInputSettings::DualMono((_, right)) = &dual.mixer_settings.analog_input else {
        panic!("fixture must use dual mono input");
    };
    let right_volume = changed(right.volume);
    checked_edit(
        &dual_bytes,
        220..221,
        |song| {
            if let AnalogInputSettings::DualMono((_, right)) = &mut song.mixer_settings.analog_input
            {
                right.volume = right_volume;
            }
        },
        |song| {
            assert!(
                matches!(&song.mixer_settings.analog_input, AnalogInputSettings::DualMono((_, right)) if right.volume == right_volume)
            )
        },
    );
    let right_mfx = changed(right.mfx);
    checked_edit(
        &dual_bytes,
        225..226,
        |song| {
            if let AnalogInputSettings::DualMono((_, right)) = &mut song.mixer_settings.analog_input
            {
                right.mfx = right_mfx;
            }
        },
        |song| {
            assert!(
                matches!(&song.mixer_settings.analog_input, AnalogInputSettings::DualMono((_, right)) if right.mfx == right_mfx)
            )
        },
    );
}

#[test]
fn effects_mapping_scales_and_grooves_round_trip() {
    let Some(bytes) = fixture(V6_FIXTURE) else {
        return;
    };
    let initial = parse(&bytes);
    let effect = changed(initial.effects_settings.chorus_mod_depth);
    checked_edit(
        &bytes,
        V4_OFFSETS.effect_settings..V4_OFFSETS.effect_settings + 1,
        |song| song.effects_settings.chorus_mod_depth = effect,
        |song| assert_eq!(song.effects_settings.chorus_mod_depth, effect),
    );
    let mapping = changed(initial.midi_mappings[0].value);
    checked_edit(
        &bytes,
        V4_OFFSETS.midi_mapping + 2..V4_OFFSETS.midi_mapping + 3,
        |song| song.midi_mappings[0].value = mapping,
        |song| assert_eq!(song.midi_mappings[0].value, mapping),
    );
    let groove = changed(initial.grooves[0].steps[0]);
    checked_edit(
        &bytes,
        V4_OFFSETS.groove..V4_OFFSETS.groove + 1,
        |song| song.grooves[0].steps[0] = groove,
        |song| assert_eq!(song.grooves[0].steps[0], groove),
    );
    if let Some(ppqn_bytes) = fixture("V6_6EMPTY.m8s") {
        let ppqn_song = parse(&ppqn_bytes);
        let ppqn = changed(ppqn_song.grooves[0].ppqn.expect("6.6 fixture has PPQN"));
        let offset = V4_1_OFFSETS.groove_ppqn_offsets.expect("PPQN offset");
        checked_edit(
            &ppqn_bytes,
            offset..offset + 1,
            |song| song.grooves[0].ppqn = Some(ppqn),
            |song| assert_eq!(song.grooves[0].ppqn, Some(ppqn)),
        );
    }

    let scale_base = V4_OFFSETS.scale;
    let semitone = changed(initial.scales[0].notes[0].semitone_byte);
    checked_edit(
        &bytes,
        scale_base + 2..scale_base + 3,
        |song| song.scales[0].notes[0].semitone_byte = semitone,
        |song| assert_eq!(song.scales[0].notes[0].semitone_byte, semitone),
    );
    let cents = changed(initial.scales[0].notes[0].cents_byte);
    checked_edit(
        &bytes,
        scale_base + 3..scale_base + 4,
        |song| song.scales[0].notes[0].cents_byte = cents,
        |song| assert_eq!(song.scales[0].notes[0].cents_byte, cents),
    );
    let enabled = !initial.scales[0].notes[0].enabled;
    checked_edit(
        &bytes,
        scale_base..scale_base + 1,
        |song| song.scales[0].notes[0].enabled = enabled,
        |song| assert_eq!(song.scales[0].notes[0].enabled, enabled),
    );
    let mut song = parse(&bytes);
    song.scales[0].name = "ZT".into();
    let output = write(&song, &bytes);
    assert_only_changed_in(&bytes, &output, scale_base + 26..scale_base + 42);
    assert_eq!(output[scale_base + 28], 0);
    assert_eq!(parse(&output).scales[0].name, "ZT");
}

#[test]
fn bookmarks_and_row_bookmarks_round_trip() {
    let Some(bytes) = fixture("V6_6EMPTY.m8s") else {
        return;
    };
    let initial = parse(&bytes);
    let bookmark = changed(initial.song.bookmarks[0]);
    checked_edit(
        &bytes,
        V4_OFFSETS.bookmarks..V4_OFFSETS.bookmarks + 1,
        |song| song.song.bookmarks[0] = bookmark,
        |song| assert_eq!(song.song.bookmarks[0], bookmark),
    );
    let row_bookmarks = initial
        .song
        .row_bookmarks
        .expect("6.6 fixture has row bookmarks");
    let row = changed(row_bookmarks[0]);
    let offset = V4_1_OFFSETS
        .row_bookmark_offset
        .expect("row bookmark offset");
    checked_edit(
        &bytes,
        offset..offset + 1,
        |song| song.song.row_bookmarks.as_mut().expect("row bookmarks")[0] = row,
        |song| assert_eq!(song.song.row_bookmarks.expect("row bookmarks")[0], row),
    );
}
