//! Optional edit/write/reread checks against a private song.
//! Set ZT_M8_TEST_SONG to a local .m8s file to run them.

use m8edit::{Command, EditError, Editor, FieldValue, Granularity, Selection, read_field};
use m8fmt::{Song, V4_OFFSETS, reader::Reader, writer::Writer};
use m8view::{Field, InstrumentParameter as P, View, field_spans};

fn fixture() -> Option<(Vec<u8>, Song)> {
    let path = std::env::var_os("ZT_M8_TEST_SONG")?;
    let bytes = std::fs::read(path).expect("test song is readable");
    let song = Song::read_from_reader(&mut Reader::new(bytes.clone())).expect("song parses");
    Some((bytes, song))
}

#[test]
fn song_name_set_undo_and_round_trip() {
    let Some((bytes, song)) = fixture() else {
        return;
    };
    let original_name = song.name.clone();
    let name = "ZT_RENAMED".to_string();
    let mut editor = Editor::new(song);
    assert_eq!(
        read_field(editor.song(), Field::SongName).unwrap(),
        FieldValue::Text(original_name.clone())
    );
    assert!(
        editor
            .apply(Command::Set {
                field: Field::SongName,
                value: FieldValue::Text(name.clone()),
            })
            .unwrap()
            .changed
    );
    assert_eq!(editor.song().name, name);
    for invalid in ["THIRTEEN_CHARS", "Café"] {
        assert_eq!(
            editor.apply(Command::Set {
                field: Field::SongName,
                value: FieldValue::Text(invalid.to_string()),
            }),
            Err(EditError::InvalidValue)
        );
        assert_eq!(editor.song().name, name);
    }
    editor.apply(Command::Undo).unwrap();
    assert_eq!(editor.song().name, original_name);
    editor.apply(Command::Redo).unwrap();
    assert_eq!(editor.song().name, name);

    let mut writer = Writer::new(bytes.clone());
    editor.song().write(&mut writer).expect("song writes");
    let encoded = writer.finish();
    let reread = Song::read_from_reader(&mut Reader::new(encoded.clone())).expect("song rereads");
    assert_eq!(reread.name, name);
    assert!(
        bytes
            .iter()
            .zip(encoded.iter())
            .enumerate()
            .all(|(offset, (before, after))| before == after || (0x94..0xA0).contains(&offset))
    );
}

#[test]
fn song_and_mixer_fields_survive_save_and_support_defaults() {
    let Some((bytes, song)) = fixture() else {
        return;
    };
    let mut editor = Editor::new(song);
    for (field, value) in [
        (Field::SongTempo, FieldValue::Tempo(137.25)),
        (Field::SongTranspose, FieldValue::Byte(0xFE)),
        (Field::MixerTrackVolume { track: 3 }, FieldValue::Byte(0x90)),
        (Field::MixerMasterVolume, FieldValue::Byte(0xB0)),
        (Field::MixerLimiter, FieldValue::Byte(0x30)),
    ] {
        assert!(editor.apply(Command::Set { field, value }).unwrap().changed);
    }
    editor
        .apply(Command::Adjust {
            field: Field::SongTranspose,
            direction: 1,
            granularity: Granularity::Fine,
        })
        .unwrap();
    assert_eq!(editor.song().transpose, 0xFF);
    editor
        .apply(Command::Adjust {
            field: Field::SongTempo,
            direction: 1,
            granularity: Granularity::Fine,
        })
        .unwrap();
    assert!((editor.song().tempo - 137.35).abs() < 0.001);

    let mut writer = Writer::new(bytes);
    editor.song().write(&mut writer).expect("song writes");
    let reread = Song::read_from_reader(&mut Reader::new(writer.finish())).expect("song rereads");
    assert!((reread.tempo - 137.35).abs() < 0.001);
    assert_eq!(reread.transpose, 0xFF);
    assert_eq!(reread.mixer_settings.track_volume[3], 0x90);
    assert_eq!(reread.mixer_settings.master_volume, 0xB0);
    assert_eq!(reread.mixer_settings.limiter.level, 0x30);

    editor
        .apply(Command::Clear {
            field: Field::SongTempo,
        })
        .unwrap();
    editor
        .apply(Command::Clear {
            field: Field::SongTranspose,
        })
        .unwrap();
    editor
        .apply(Command::Clear {
            field: Field::MixerTrackVolume { track: 3 },
        })
        .unwrap();
    editor
        .apply(Command::Clear {
            field: Field::MixerMasterVolume,
        })
        .unwrap();
    assert_eq!(editor.song().tempo, 120.0);
    assert_eq!(editor.song().transpose, 0);
    assert_eq!(editor.song().mixer_settings.track_volume[3], 0xE0);
    assert_eq!(editor.song().mixer_settings.master_volume, 0xE0);
    assert_eq!(
        editor.apply(Command::Set {
            field: Field::SongTempo,
            value: FieldValue::Tempo(f32::NAN),
        }),
        Err(EditError::InvalidValue)
    );
    assert_eq!(
        editor.apply(Command::Set {
            field: Field::MixerTrackVolume { track: 8 },
            value: FieldValue::Byte(0x80),
        }),
        Err(EditError::OutOfRange)
    );
}

#[test]
fn phrase_note_changes_only_its_byte_and_survives_reread() {
    let Some((bytes, song)) = fixture() else {
        return;
    };
    let original = song.phrases[0].steps[0].note.0;
    let new = if original == 48 { 49 } else { 48 };
    let mut editor = Editor::new(song);
    let field = Field::PhraseNote { phrase: 0, row: 0 };
    assert!(
        editor
            .apply(Command::Set {
                field,
                value: FieldValue::Byte(new)
            })
            .unwrap()
            .changed
    );
    assert!(editor.is_dirty());
    let mut writer = Writer::new(bytes.clone());
    editor.song().write(&mut writer).expect("song writes");
    let output = writer.finish();
    let changed = bytes
        .iter()
        .zip(&output)
        .enumerate()
        .filter_map(|(index, (old, new))| (old != new).then_some(index))
        .collect::<Vec<_>>();
    assert_eq!(changed, vec![V4_OFFSETS.phrases]);
    let reread = Song::read_from_reader(&mut Reader::new(output)).expect("song rereads");
    assert_eq!(reread.phrases[0].steps[0].note.0, new);
}

#[test]
fn note_steps_and_undo_redo_cross_savepoint() {
    let Some((_, song)) = fixture() else {
        return;
    };
    let mut editor = Editor::new(song);
    let field = Field::PhraseNote { phrase: 0, row: 0 };
    editor
        .apply(Command::Set {
            field,
            value: FieldValue::Byte(36),
        })
        .unwrap();
    editor.mark_saved();
    editor
        .apply(Command::Adjust {
            field,
            direction: 1,
            granularity: Granularity::Coarse,
        })
        .unwrap();
    assert_eq!(editor.song().phrases[0].steps[0].note.0, 48);
    assert!(editor.is_dirty());
    editor.apply(Command::Undo).unwrap();
    assert_eq!(editor.song().phrases[0].steps[0].note.0, 36);
    assert!(!editor.is_dirty());
    editor.apply(Command::Redo).unwrap();
    assert_eq!(editor.song().phrases[0].steps[0].note.0, 48);
    assert!(editor.is_dirty());
}

#[test]
fn replace_instrument_and_optional_table_is_one_undoable_edit() {
    let Some((_, song)) = fixture() else {
        return;
    };
    let Some(slot) = song
        .instruments
        .iter()
        .position(|instrument| !matches!(instrument, m8fmt::Instrument::None))
    else {
        return;
    };
    let original_instrument = song.instruments[slot].clone();
    let original_table = song.tables[slot].clone();
    let mut replacement_table = original_table.clone();
    replacement_table.steps[0].transpose ^= 1;
    let mut editor = Editor::new(song);

    assert!(
        editor
            .apply(Command::ReplaceInstrument {
                slot: slot as u8,
                instrument: Box::new(m8fmt::Instrument::None),
                table: None,
            })
            .unwrap()
            .changed
    );
    let replaced_instrument = editor.song().instruments[slot].clone();
    assert!(matches!(replaced_instrument, m8fmt::Instrument::None));
    assert!(editor.song().tables[slot] == original_table);
    assert!(editor.is_dirty());

    editor.apply(Command::Undo).unwrap();
    assert_eq!(editor.song().instruments[slot], original_instrument);
    assert!(editor.song().tables[slot] == original_table);
    assert!(!editor.is_dirty());

    editor.apply(Command::Redo).unwrap();
    assert_eq!(editor.song().instruments[slot], replaced_instrument);
    assert!(editor.is_dirty());

    editor
        .apply(Command::ReplaceInstrument {
            slot: slot as u8,
            instrument: Box::new(original_instrument.clone()),
            table: Some(Box::new(replacement_table.clone())),
        })
        .unwrap();
    assert!(editor.song().tables[slot] == replacement_table);
    editor.apply(Command::Undo).unwrap();
    assert_eq!(editor.song().instruments[slot], replaced_instrument);
    assert!(editor.song().tables[slot] == original_table);
    editor.apply(Command::Redo).unwrap();
    assert_eq!(editor.song().instruments[slot], original_instrument);
    assert!(editor.song().tables[slot] == replacement_table);
}

#[test]
fn instrument_type_load_and_save_actions_are_not_value_edits() {
    let Some((_, song)) = fixture() else {
        return;
    };
    let Some(index) = song
        .instruments
        .iter()
        .position(|instrument| !matches!(instrument, m8fmt::Instrument::None))
    else {
        return;
    };
    let mut editor = Editor::new(song);
    for parameter in [P::Kind, P::Load, P::Save, P::SampleEdit] {
        let field = Field::Instrument {
            index: index as u8,
            parameter,
        };
        assert_eq!(
            read_field(editor.song(), field),
            Err(EditError::UnsupportedOperation)
        );
        assert_eq!(
            editor.apply(Command::Set {
                field,
                value: FieldValue::Byte(0),
            }),
            Err(EditError::UnsupportedOperation)
        );
    }
}

#[test]
fn fx_value_steps_by_one_or_sixteen() {
    let Some((_, song)) = fixture() else {
        return;
    };
    let mut editor = Editor::new(song);
    let field = Field::TableFxValue {
        table: 1,
        row: 0,
        lane: 0,
    };
    editor
        .apply(Command::Set {
            field,
            value: FieldValue::Byte(0x20),
        })
        .unwrap();
    editor
        .apply(Command::Adjust {
            field,
            direction: 1,
            granularity: Granularity::Fine,
        })
        .unwrap();
    editor
        .apply(Command::Adjust {
            field,
            direction: -1,
            granularity: Granularity::Coarse,
        })
        .unwrap();
    assert_eq!(editor.song().tables[1].steps[0].fx1.value, 0x11);
}

#[test]
fn inserting_note_fills_empty_velocity_and_instrument() {
    let Some((_, song)) = fixture() else {
        return;
    };
    let step = &song.phrases[0].steps[3];
    assert_eq!(
        (step.note.0, step.velocity, step.instrument),
        (0xFF, 0xFF, 0xFF)
    );
    let mut editor = Editor::new(song);
    editor
        .apply(Command::InsertLast {
            field: Field::PhraseNote { phrase: 0, row: 3 },
        })
        .unwrap();
    let step = &editor.song().phrases[0].steps[3];
    assert_eq!((step.note.0, step.velocity, step.instrument), (36, 0x64, 0));
    editor.apply(Command::Undo).unwrap();
    let step = &editor.song().phrases[0].steps[3];
    assert_eq!(
        (step.note.0, step.velocity, step.instrument),
        (0xFF, 0xFF, 0xFF)
    );
}

#[test]
fn create_chain_and_clone_phrase_survive_roundtrip() {
    let Some((bytes, song)) = fixture() else {
        return;
    };
    let source_phrase = song.chains[usize::from(song.song.steps[0])].steps[0].phrase;
    let mut editor = Editor::new(song);
    let chain = editor
        .apply(Command::CreateChain {
            row: 3,
            track: 0,
            clone_from: None,
            clone_phrases: false,
        })
        .unwrap()
        .assigned_index
        .unwrap();
    let phrase = editor
        .apply(Command::CreatePhrase {
            chain,
            row: 0,
            clone_from: Some(source_phrase),
        })
        .unwrap()
        .assigned_index
        .unwrap();
    assert_ne!(phrase, source_phrase);
    assert_eq!(editor.song().song.steps[3 * 8], chain);
    assert_eq!(
        editor.song().chains[usize::from(chain)].steps[0].phrase,
        phrase
    );
    assert_eq!(
        editor.song().phrases[usize::from(phrase)].steps,
        editor.song().phrases[usize::from(source_phrase)].steps
    );

    let mut writer = Writer::new(bytes);
    editor.song().write(&mut writer).unwrap();
    let reread = Song::read_from_reader(&mut Reader::new(writer.finish())).unwrap();
    assert_eq!(reread.song.steps[3 * 8], chain);
    assert_eq!(reread.chains[usize::from(chain)].steps[0].phrase, phrase);
    assert_eq!(
        reread.phrases[usize::from(phrase)].steps,
        reread.phrases[usize::from(source_phrase)].steps
    );

    editor.apply(Command::Undo).unwrap();
    assert!(editor.song().chains[usize::from(chain)].is_empty());
    editor.apply(Command::Undo).unwrap();
    assert_eq!(editor.song().song.steps[3 * 8], 0xFF);
}

#[test]
fn rectangular_copy_paste_is_one_undo_transaction() {
    let Some((_, song)) = fixture() else {
        return;
    };
    let mut editor = Editor::new(song);
    let source = editor.song().phrases[0].steps[0].clone();
    let old_destination = editor.song().phrases[0].steps[5].clone();
    editor
        .apply(Command::Copy {
            selection: Selection {
                anchor: Field::PhraseNote { phrase: 0, row: 0 },
                focus: Field::PhraseInstrument { phrase: 0, row: 0 },
            },
        })
        .unwrap();
    assert!(!editor.is_dirty());
    editor
        .apply(Command::Paste {
            anchor: Field::PhraseNote { phrase: 0, row: 5 },
        })
        .unwrap();
    let destination = &editor.song().phrases[0].steps[5];
    assert_eq!(
        (
            destination.note,
            destination.velocity,
            destination.instrument
        ),
        (source.note, source.velocity, source.instrument)
    );
    editor.apply(Command::Undo).unwrap();
    assert_eq!(editor.song().phrases[0].steps[5], old_destination);
}

#[test]
fn out_of_bounds_paste_does_not_partially_mutate_song() {
    let Some((_, song)) = fixture() else {
        return;
    };
    let mut editor = Editor::new(song);
    editor
        .apply(Command::Copy {
            selection: Selection {
                anchor: Field::PhraseNote { phrase: 0, row: 0 },
                focus: Field::PhraseNote { phrase: 0, row: 1 },
            },
        })
        .unwrap();
    let before = editor.song().clone();
    assert_eq!(
        editor.apply(Command::Paste {
            anchor: Field::PhraseNote { phrase: 0, row: 15 }
        }),
        Err(EditError::OutOfRange)
    );
    assert!(editor.song() == &before);
}

#[test]
fn instrument_fields_survive_write_and_reread() {
    let Some((bytes, song)) = fixture() else {
        return;
    };
    let mut editor = Editor::new(song);
    let changes = [
        (0, P::Name, FieldValue::Text("ZT SAMPLER".into())),
        (0, P::Play, FieldValue::Byte(1)),
        (0, P::Slice, FieldValue::Byte(2)),
        (0, P::Cutoff, FieldValue::Byte(0xA0)),
        (1, P::FmAlgorithm, FieldValue::Byte(3)),
        (1, P::FmWave(0), FieldValue::Byte(2)),
        (1, P::FmRatio(0), FieldValue::Byte(2)),
        (3, P::MacroShape, FieldValue::Byte(4)),
        (3, P::MacroTimbre, FieldValue::Byte(0x55)),
    ];
    for (index, parameter, value) in &changes {
        editor
            .apply(Command::Set {
                field: Field::Instrument {
                    index: *index,
                    parameter: *parameter,
                },
                value: value.clone(),
            })
            .unwrap();
    }
    let mut writer = Writer::new(bytes);
    editor.song().write(&mut writer).unwrap();
    let reread = Song::read_from_reader(&mut Reader::new(writer.finish())).unwrap();
    for (index, parameter, value) in &changes {
        assert_eq!(
            m8edit::read_field(
                &reread,
                Field::Instrument {
                    index: *index,
                    parameter: *parameter,
                }
            )
            .unwrap(),
            *value
        );
    }
}

#[test]
fn every_displayed_instrument_field_is_editable() {
    let Some((_, song)) = fixture() else {
        return;
    };
    let mut editor = Editor::new(song);
    for index in [0, 1, 3] {
        let spans = field_spans(editor.song(), View::Instrument { index }).unwrap();
        assert!(!spans.is_empty());
        for span in spans {
            let value = m8edit::read_field(editor.song(), span.field).unwrap();
            editor
                .apply(Command::Set {
                    field: span.field,
                    value,
                })
                .unwrap();
        }
    }
    assert!(!editor.is_dirty());
}

#[test]
fn instrument_selection_copies_only_to_same_type() {
    let Some((_, song)) = fixture() else {
        return;
    };
    let mut editor = Editor::new(song);
    let original_destination = editor.song().instruments[2].clone();
    let source = Field::Instrument {
        index: 1,
        parameter: P::FmAlgorithm,
    };
    let destination = Field::Instrument {
        index: 2,
        parameter: P::FmAlgorithm,
    };
    editor
        .apply(Command::Copy {
            selection: Selection {
                anchor: source,
                focus: Field::Instrument {
                    index: 1,
                    parameter: P::FmWave(1),
                },
            },
        })
        .unwrap();
    editor
        .apply(Command::Paste {
            anchor: destination,
        })
        .unwrap();
    assert_eq!(
        m8edit::read_field(editor.song(), destination).unwrap(),
        m8edit::read_field(editor.song(), source).unwrap()
    );
    editor.apply(Command::Undo).unwrap();
    assert_eq!(editor.song().instruments[2], original_destination);
    assert_eq!(
        editor.apply(Command::Paste {
            anchor: Field::Instrument {
                index: 0,
                parameter: P::Play,
            },
        }),
        Err(EditError::IncompatibleSelection)
    );
}

#[test]
fn sampler_slice_changes_only_its_own_byte() {
    let Some((bytes, song)) = fixture() else {
        return;
    };
    let original = m8edit::read_field(
        &song,
        Field::Instrument {
            index: 0,
            parameter: P::Slice,
        },
    )
    .unwrap();
    let FieldValue::Byte(original) = original else {
        panic!("slice is a byte")
    };
    let new = if original == 0 { 1 } else { 0 };
    let mut editor = Editor::new(song);
    editor
        .apply(Command::Set {
            field: Field::Instrument {
                index: 0,
                parameter: P::Slice,
            },
            value: FieldValue::Byte(new),
        })
        .unwrap();
    let mut writer = Writer::new(bytes.clone());
    editor.song().write(&mut writer).unwrap();
    let output = writer.finish();
    let changed = bytes
        .iter()
        .zip(&output)
        .enumerate()
        .filter_map(|(index, (old, new))| (old != new).then_some(index))
        .collect::<Vec<_>>();
    assert_eq!(changed, vec![V4_OFFSETS.instruments + 19]);
    let reread = Song::read_from_reader(&mut Reader::new(output)).unwrap();
    assert_eq!(
        m8edit::read_field(
            &reread,
            Field::Instrument {
                index: 0,
                parameter: P::Slice,
            }
        )
        .unwrap(),
        FieldValue::Byte(new)
    );
}
