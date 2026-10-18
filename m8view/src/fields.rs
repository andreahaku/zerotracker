//! Screen coordinates and command choices for editable values.

use crate::{Field, FieldSpan, InstrumentParameter as P, View, ViewError, render};
use m8fmt::{CommandPack, FX, Instrument, Song};

fn span(out: &mut Vec<FieldSpan>, field: Field, line: usize, column: usize, width: usize) {
    out.push(FieldSpan {
        field,
        line,
        column,
        width,
    });
}

/// Editable values in visual order, with coordinates in the complete frame.
pub fn field_spans(song: &Song, view: View) -> Result<Vec<FieldSpan>, ViewError> {
    render(song, view)?;
    let mut out = Vec::new();
    match view {
        View::Song { top_row } => {
            let start = usize::from(top_row).min(240);
            for visible_row in 0..16 {
                for track in 0..8 {
                    span(
                        &mut out,
                        Field::SongChain {
                            row: (start + visible_row) as u8,
                            track,
                        },
                        2 + visible_row,
                        4 + usize::from(track) * 3,
                        2,
                    );
                }
            }
        }
        View::Chain { index } => {
            for row in 0..16 {
                span(
                    &mut out,
                    Field::ChainPhrase { chain: index, row },
                    2 + usize::from(row),
                    3,
                    2,
                );
                span(
                    &mut out,
                    Field::ChainTranspose { chain: index, row },
                    2 + usize::from(row),
                    6,
                    2,
                );
            }
        }
        View::Phrase { index } => {
            for row in 0..16 {
                let line = 2 + usize::from(row);
                span(
                    &mut out,
                    Field::PhraseNote { phrase: index, row },
                    line,
                    3,
                    3,
                );
                span(
                    &mut out,
                    Field::PhraseVelocity { phrase: index, row },
                    line,
                    7,
                    2,
                );
                span(
                    &mut out,
                    Field::PhraseInstrument { phrase: index, row },
                    line,
                    10,
                    2,
                );
                for lane in 0..3 {
                    let column = 13 + usize::from(lane) * 6;
                    span(
                        &mut out,
                        Field::PhraseFxCommand {
                            phrase: index,
                            row,
                            lane,
                        },
                        line,
                        column,
                        3,
                    );
                    span(
                        &mut out,
                        Field::PhraseFxValue {
                            phrase: index,
                            row,
                            lane,
                        },
                        line,
                        column + 3,
                        2,
                    );
                }
            }
        }
        View::Table { index } => {
            for row in 0..16 {
                let line = 2 + usize::from(row);
                span(
                    &mut out,
                    Field::TableTranspose { table: index, row },
                    line,
                    3,
                    2,
                );
                span(
                    &mut out,
                    Field::TableVelocity { table: index, row },
                    line,
                    6,
                    2,
                );
                for lane in 0..3 {
                    let column = 9 + usize::from(lane) * 6;
                    span(
                        &mut out,
                        Field::TableFxCommand {
                            table: index,
                            row,
                            lane,
                        },
                        line,
                        column,
                        3,
                    );
                    span(
                        &mut out,
                        Field::TableFxValue {
                            table: index,
                            row,
                            lane,
                        },
                        line,
                        column + 3,
                        2,
                    );
                }
            }
        }
        View::Instrument { index } => instrument_spans(song, index, &mut out),
    }
    Ok(out)
}

/// The right-hand column of the synth pages, from AMP down to REV.
const MIXER_COLUMN: [P; 7] = [
    P::Amp,
    P::Limit,
    P::Pan,
    P::Dry,
    P::ModFx,
    P::Delay,
    P::Reverb,
];

fn instrument_spans(song: &Song, index: u8, out: &mut Vec<FieldSpan>) {
    let add = |out: &mut Vec<FieldSpan>, parameter, line, column, width| {
        span(
            out,
            Field::Instrument { index, parameter },
            line,
            column,
            width,
        );
    };
    let instrument = &song.instruments[usize::from(index)];
    add(out, P::Kind, 1, 9, 8);
    if matches!(instrument, Instrument::None) {
        return;
    }
    add(out, P::Load, 1, 23, 4);
    add(out, P::Save, 1, 28, 4);
    add(out, P::Name, 2, 9, 13);
    add(out, P::Transpose, 3, 9, 3);
    add(out, P::TableTick, 3, 22, 2);
    if matches!(instrument, Instrument::MIDIOut(_) | Instrument::External(_)) {
        return;
    }
    add(out, P::Eq, 3, 30, 2);

    match instrument {
        Instrument::Sampler(sampler) => {
            add(out, P::SamplePath, 5, 9, 16);
            // EDIT, or REC. while there is no sample.
            add(out, P::SampleEdit, 5, 26, 4);
            add(out, P::Slice, 7, 9, 2);
            add(out, P::Amp, 7, 23, 2);
            add(out, P::Play, 8, 9, 2);
            add(out, P::Limit, 8, 23, 2);
            let bpm_mode = matches!(
                sampler.play_mode,
                m8fmt::SamplePlayMode::REP_BPM
                    | m8fmt::SamplePlayMode::BPM_REV
                    | m8fmt::SamplePlayMode::BPM_PP
            );
            let left = if bpm_mode {
                [P::Detune, P::Start, P::LoopStart, P::Length, P::Degrade]
            } else {
                [P::Start, P::LoopStart, P::Length, P::Detune, P::Degrade]
            };
            let right = [P::Pan, P::Dry, P::ModFx, P::Delay, P::Reverb];
            for offset in 0..5 {
                add(out, left[offset], 9 + offset, 9, 2);
                add(out, right[offset], 9 + offset, 23, 2);
            }
            for (line, parameter) in [(14, P::Filter), (15, P::Cutoff), (16, P::Resonance)] {
                add(out, parameter, line, 9, 2);
            }
        }
        Instrument::FMSynth(_) => {
            add(out, P::FmAlgorithm, 5, 9, 2);
            for operator in 0..4 {
                let column = 9 + usize::from(operator) * 6;
                add(out, P::FmWave(operator), 6, column + 2, 3);
                add(out, P::FmRatio(operator), 7, column, 2);
                add(out, P::FmRatioFine(operator), 7, column + 3, 2);
                add(out, P::FmLevel(operator), 8, column, 2);
                add(out, P::FmFeedback(operator), 8, column + 3, 2);
                add(out, P::FmModA(operator), 9, column, 5);
                add(out, P::FmModB(operator), 10, column, 5);
            }
            let left = [
                P::FmMod(0),
                P::FmMod(1),
                P::FmMod(2),
                P::FmMod(3),
                P::Filter,
                P::Cutoff,
                P::Resonance,
            ];
            let right = [
                P::Amp,
                P::Limit,
                P::Pan,
                P::Dry,
                P::ModFx,
                P::Delay,
                P::Reverb,
            ];
            for offset in 0..7 {
                add(out, left[offset], 11 + offset, 9, 2);
                add(out, right[offset], 11 + offset, 23, 2);
            }
        }
        Instrument::MacroSynth(macro_synth) => {
            let shape_width = 2 + format!("{:?}", macro_synth.shape).chars().count();
            add(out, P::MacroShape, 5, 9, shape_width);
            let left = [
                P::MacroTimbre,
                P::MacroColor,
                P::Degrade,
                P::MacroRedux,
                P::Filter,
                P::Cutoff,
                P::Resonance,
            ];
            let right = [
                P::Amp,
                P::Limit,
                P::Pan,
                P::Dry,
                P::ModFx,
                P::Delay,
                P::Reverb,
            ];
            for offset in 0..7 {
                add(out, left[offset], 7 + offset, 9, 2);
                add(out, right[offset], 7 + offset, 23, 2);
            }
        }
        Instrument::WavSynth(_) => {
            // The number only: the name after it reads like FILTER's.
            add(out, P::WavShape, 5, 9, 2);
            let left = [
                P::WavSize,
                P::WavMult,
                P::WavWarp,
                P::WavScan,
                P::Filter,
                P::Cutoff,
                P::Resonance,
            ];
            for offset in 0..7 {
                add(out, left[offset], 7 + offset, 9, 2);
                add(out, MIXER_COLUMN[offset], 7 + offset, 23, 2);
            }
        }
        Instrument::HyperSynth(hyper) => {
            add(out, P::HyperScale, 5, 9, 2);
            add(out, P::HyperChord, 6, 9, 2);
            for k in 0..6u8 {
                add(out, P::HyperInterval(k), 6, 13 + 3 * usize::from(k), 2);
            }
            if hyper.shape.is_some() {
                add(out, P::HyperShape, 9, 9, 2);
            }
            let left = [
                P::HyperShift,
                P::HyperSwarm,
                P::HyperWidth,
                P::HyperSubosc,
                P::Filter,
                P::Cutoff,
                P::Resonance,
            ];
            for offset in 0..7 {
                add(out, left[offset], 10 + offset, 9, 2);
                add(out, MIXER_COLUMN[offset], 10 + offset, 23, 2);
            }
        }
        _ => {}
    }
    if let Some(params) = crate::mod_params(instrument) {
        for (slot, m) in params.mods.iter().enumerate() {
            let (kind, _) = crate::mod_bytes(m);
            let top = crate::MODS_LINE + (slot % 2) * crate::MODS_PAIR_GAP;
            let column = if slot < 2 { 9 } else { crate::MODS_RIGHT_VALUE };
            let slot = slot as u8;
            add(out, P::ModType(slot), top, column, 2);
            add(out, P::ModDest(slot), top + 1, column, 2);
            let labels = crate::MOD_PARAM_LABELS
                .get(usize::from(kind))
                .copied()
                .unwrap_or_default();
            for (k, label) in labels.iter().enumerate() {
                if !label.is_empty() {
                    add(out, P::ModParam(slot, k as u8), top + 2 + k, column, 2);
                }
            }
        }
    }
    out.sort_by_key(|field| (field.line, field.column));
}

/// Command IDs and three-letter names valid for an FX command cell.
/// Empty results mean the field is not an FX command cell or is out of range.
pub fn fx_commands(song: &Song, field: Field) -> Vec<(u8, &'static str)> {
    let instrument = match field {
        Field::PhraseFxCommand { phrase, row, lane } if lane < 3 && row < 16 => {
            let Some(phrase) = song.phrases.get(usize::from(phrase)) else {
                return Vec::new();
            };
            let mut instrument = None;
            for step in phrase.steps.iter().take(usize::from(row) + 1) {
                if let Some(current) = song.instruments.get(usize::from(step.instrument)) {
                    instrument = Some(current);
                }
            }
            instrument
        }
        Field::TableFxCommand { table, row, lane }
            if lane < 3 && row < 16 && song.tables.get(usize::from(table)).is_some() =>
        {
            song.instruments.get(usize::from(table))
        }
        _ => return Vec::new(),
    };
    let pack = instrument
        .unwrap_or(&Instrument::None)
        .instr_command_text(song.version);
    command_choices(song, pack)
}

fn command_choices(song: &Song, pack: CommandPack) -> Vec<(u8, &'static str)> {
    let mut commands = FX::fx_command_names(song.version)
        .commands
        .iter()
        .enumerate()
        .map(|(id, &name)| (id as u8, name))
        .collect::<Vec<_>>();
    for id in 0x80..=0xFE {
        if let Some(name) = pack.try_render(id) {
            commands.push((id, name));
        }
    }
    commands
}
