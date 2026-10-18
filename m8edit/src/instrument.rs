use crate::{EditError, FieldValue};
use m8fmt::{
    FMWave, FmAlgo, HyperSynthShape, Instrument, LimitType, MacroSynthOsc, SamplePlayMode, Song,
    SynthParams, WavShape,
};
use m8view::{Field, InstrumentParameter as P};

fn parts(instrument: &Instrument) -> Result<(&String, bool, u8, &SynthParams), EditError> {
    match instrument {
        Instrument::Sampler(s) => Ok((&s.name, s.transpose, s.table_tick, &s.synth_params)),
        Instrument::FMSynth(s) => Ok((&s.name, s.transpose, s.table_tick, &s.synth_params)),
        Instrument::MacroSynth(s) => Ok((&s.name, s.transpose, s.table_tick, &s.synth_params)),
        Instrument::WavSynth(s) => Ok((&s.name, s.transpose, s.table_tick, &s.synth_params)),
        Instrument::HyperSynth(s) => Ok((&s.name, s.transpose, s.table_tick, &s.synth_params)),
        _ => Err(EditError::UnsupportedOperation),
    }
}

fn parts_mut(
    instrument: &mut Instrument,
) -> Result<(&mut String, &mut bool, &mut u8, &mut SynthParams), EditError> {
    match instrument {
        Instrument::Sampler(s) => Ok((
            &mut s.name,
            &mut s.transpose,
            &mut s.table_tick,
            &mut s.synth_params,
        )),
        Instrument::FMSynth(s) => Ok((
            &mut s.name,
            &mut s.transpose,
            &mut s.table_tick,
            &mut s.synth_params,
        )),
        Instrument::MacroSynth(s) => Ok((
            &mut s.name,
            &mut s.transpose,
            &mut s.table_tick,
            &mut s.synth_params,
        )),
        Instrument::WavSynth(s) => Ok((
            &mut s.name,
            &mut s.transpose,
            &mut s.table_tick,
            &mut s.synth_params,
        )),
        Instrument::HyperSynth(s) => Ok((
            &mut s.name,
            &mut s.transpose,
            &mut s.table_tick,
            &mut s.synth_params,
        )),
        _ => Err(EditError::UnsupportedOperation),
    }
}

fn target(song: &Song, field: Field) -> Result<(&Instrument, P), EditError> {
    let Field::Instrument { index, parameter } = field else {
        return Err(EditError::UnsupportedOperation);
    };
    if matches!(parameter, P::Kind | P::Load | P::Save | P::SampleEdit) {
        return Err(EditError::UnsupportedOperation);
    }
    let instrument = song
        .instruments
        .get(usize::from(index))
        .ok_or(EditError::OutOfRange)?;
    Ok((instrument, parameter))
}

fn target_mut(song: &mut Song, field: Field) -> Result<(&mut Instrument, P), EditError> {
    let Field::Instrument { index, parameter } = field else {
        return Err(EditError::UnsupportedOperation);
    };
    let instrument = song
        .instruments
        .get_mut(usize::from(index))
        .ok_or(EditError::OutOfRange)?;
    Ok((instrument, parameter))
}

/// NAME, TRANSP. and TBL.TIC of MIDI OUT and EXTERNAL: their only fields.
fn unplayed_common(instrument: &mut Instrument) -> Option<(&mut String, &mut bool, &mut u8)> {
    match instrument {
        Instrument::MIDIOut(s) => Some((&mut s.name, &mut s.transpose, &mut s.table_tick)),
        Instrument::External(s) => Some((&mut s.name, &mut s.transpose, &mut s.table_tick)),
        _ => None,
    }
}

pub(crate) fn read_field(song: &Song, field: Field) -> Result<FieldValue, EditError> {
    let (instrument, parameter) = target(song, field)?;
    read_parameter(instrument, parameter)
}

/// One parameter of an instrument, as `read_field` gives it for a song's slot.
pub fn read_parameter(instrument: &Instrument, parameter: P) -> Result<FieldValue, EditError> {
    if matches!(parameter, P::Kind | P::Load | P::Save | P::SampleEdit) {
        return Err(EditError::UnsupportedOperation);
    }
    match (instrument, parameter) {
        (Instrument::MIDIOut(s), P::Name) => return Ok(FieldValue::Text(s.name.clone())),
        (Instrument::MIDIOut(s), P::Transpose) => return Ok(FieldValue::Bool(s.transpose)),
        (Instrument::MIDIOut(s), P::TableTick) => return Ok(FieldValue::Byte(s.table_tick)),
        (Instrument::External(s), P::Name) => return Ok(FieldValue::Text(s.name.clone())),
        (Instrument::External(s), P::Transpose) => return Ok(FieldValue::Bool(s.transpose)),
        (Instrument::External(s), P::TableTick) => return Ok(FieldValue::Byte(s.table_tick)),
        (Instrument::MIDIOut(_) | Instrument::External(_), _) => {
            return Err(EditError::UnsupportedOperation);
        }
        _ => {}
    }
    let (name, transpose, table_tick, synth) = parts(instrument)?;
    let common = match parameter {
        P::Name => return Ok(FieldValue::Text(name.clone())),
        P::Transpose => return Ok(FieldValue::Bool(transpose)),
        P::TableTick => Some(table_tick),
        P::Eq => Some(synth.associated_eq),
        P::Detune => Some(synth.fine_tune),
        P::Filter => Some(synth.filter_type),
        P::Cutoff => Some(synth.filter_cutoff),
        P::Resonance => Some(synth.filter_res),
        P::Amp => Some(synth.amp),
        P::Limit => Some(synth.limit.id()),
        P::Pan => Some(synth.mixer_pan),
        P::Dry => Some(synth.mixer_dry),
        P::ModFx => Some(synth.mixer_mfx),
        P::Delay => Some(synth.mixer_delay),
        P::Reverb => Some(synth.mixer_reverb),
        _ => None,
    };
    if let Some(value) = common {
        return Ok(FieldValue::Byte(value));
    }
    if let P::ModType(slot) | P::ModDest(slot) | P::ModParam(slot, _) = parameter {
        let m = synth
            .mods
            .get(usize::from(slot))
            .ok_or(EditError::OutOfRange)?;
        let (kind, bytes) = m8view::mod_bytes(m);
        return Ok(FieldValue::Byte(match parameter {
            P::ModType(_) => kind,
            P::ModDest(_) => bytes[0] & 0x0F,
            P::ModParam(_, k) if k < 5 => bytes[1 + usize::from(k)],
            _ => return Err(EditError::OutOfRange),
        }));
    }
    let value = match instrument {
        Instrument::Sampler(s) => match parameter {
            P::SamplePath => return Ok(FieldValue::Text(s.sample_path.clone())),
            P::Slice => s.slice,
            P::Play => s.play_mode.into(),
            P::Start => s.start,
            P::LoopStart => s.loop_start,
            P::Length => s.length,
            P::Degrade => s.degrade,
            _ => return Err(EditError::UnsupportedOperation),
        },
        Instrument::FMSynth(s) => match parameter {
            P::FmAlgorithm => s.algo.0,
            P::FmMod(id) => *[s.mod1, s.mod2, s.mod3, s.mod4]
                .get(usize::from(id))
                .ok_or(EditError::OutOfRange)?,
            _ => {
                let (id, kind) =
                    operator_parameter(parameter).ok_or(EditError::UnsupportedOperation)?;
                let op = s
                    .operators
                    .get(usize::from(id))
                    .ok_or(EditError::OutOfRange)?;
                match kind {
                    OpKind::Wave => op.shape.into(),
                    OpKind::Ratio => op.ratio,
                    OpKind::RatioFine => op.ratio_fine,
                    OpKind::Level => op.level,
                    OpKind::Feedback => op.feedback,
                    OpKind::ModA => op.mod_a,
                    OpKind::ModB => op.mod_b,
                }
            }
        },
        Instrument::MacroSynth(s) => match parameter {
            P::MacroShape => s.shape.into(),
            P::MacroTimbre => s.timbre,
            P::MacroColor => s.color,
            P::Degrade => s.degrade,
            P::MacroRedux => s.redux,
            _ => return Err(EditError::UnsupportedOperation),
        },
        Instrument::WavSynth(s) => match parameter {
            P::WavShape => s.shape.into(),
            P::WavSize => s.size,
            P::WavMult => s.mult,
            P::WavWarp => s.warp,
            P::WavScan => s.scan,
            _ => return Err(EditError::UnsupportedOperation),
        },
        Instrument::HyperSynth(s) => match parameter {
            P::HyperScale => s.scale,
            P::HyperChord => s.default_chord[0] & 0x0F,
            P::HyperInterval(k) => {
                let chord = &s.chords[usize::from(s.default_chord[0] & 0x0F)];
                let k = usize::from(k);
                if k >= 6 {
                    return Err(EditError::OutOfRange);
                }
                if chord.is_osc_on(k) {
                    chord.offsets[k]
                } else {
                    INTERVAL_OFF
                }
            }
            P::HyperShape => s.shape.ok_or(EditError::UnsupportedOperation)?.into(),
            P::HyperShift => s.shift,
            P::HyperSwarm => s.swarm,
            P::HyperWidth => s.width,
            P::HyperSubosc => s.subosc,
            _ => return Err(EditError::UnsupportedOperation),
        },
        _ => return Err(EditError::UnsupportedOperation),
    };
    Ok(FieldValue::Byte(value))
}

#[derive(Clone, Copy)]
enum OpKind {
    Wave,
    Ratio,
    RatioFine,
    Level,
    Feedback,
    ModA,
    ModB,
}

fn operator_parameter(parameter: P) -> Option<(u8, OpKind)> {
    Some(match parameter {
        P::FmWave(id) => (id, OpKind::Wave),
        P::FmRatio(id) => (id, OpKind::Ratio),
        P::FmRatioFine(id) => (id, OpKind::RatioFine),
        P::FmLevel(id) => (id, OpKind::Level),
        P::FmFeedback(id) => (id, OpKind::Feedback),
        P::FmModA(id) => (id, OpKind::ModA),
        P::FmModB(id) => (id, OpKind::ModB),
        _ => return None,
    })
}

pub(super) fn valid_text(text: &str, max: usize) -> bool {
    text.len() <= max && text.bytes().all(|byte| (0x20..=0x7E).contains(&byte))
}

pub(crate) fn write_field(
    song: &mut Song,
    field: Field,
    value: FieldValue,
) -> Result<(), EditError> {
    let (instrument, parameter) = target_mut(song, field)?;
    if let Some((name, transpose, table_tick)) = unplayed_common(instrument) {
        return match (parameter, value) {
            (P::Name, FieldValue::Text(text)) if valid_text(&text, 12) => {
                *name = text;
                Ok(())
            }
            (P::Transpose, FieldValue::Bool(value)) => {
                *transpose = value;
                Ok(())
            }
            (P::TableTick, FieldValue::Byte(value)) => {
                *table_tick = value;
                Ok(())
            }
            (P::Name | P::Transpose | P::TableTick, _) => Err(EditError::InvalidValue),
            _ => Err(EditError::UnsupportedOperation),
        };
    }
    match (parameter, value) {
        (P::Kind | P::Load | P::Save | P::SampleEdit, _) => Err(EditError::UnsupportedOperation),
        (P::Name, FieldValue::Text(text)) if valid_text(&text, 12) => {
            *parts_mut(instrument)?.0 = text;
            Ok(())
        }
        (P::SamplePath, FieldValue::Text(text)) if valid_text(&text, 128) => {
            let Instrument::Sampler(s) = instrument else {
                return Err(EditError::UnsupportedOperation);
            };
            s.sample_path = text;
            Ok(())
        }
        (P::Transpose, FieldValue::Bool(value)) => {
            *parts_mut(instrument)?.1 = value;
            Ok(())
        }
        (parameter, FieldValue::Byte(value)) => write_byte(instrument, parameter, value),
        _ => Err(EditError::InvalidValue),
    }
}

fn write_byte(instrument: &mut Instrument, parameter: P, value: u8) -> Result<(), EditError> {
    let (min, max, _, _) = byte_range(instrument, parameter)?;
    let off = matches!(parameter, P::HyperInterval(_)) && value == INTERVAL_OFF;
    if !off && (value < min || value > max) {
        return Err(EditError::InvalidValue);
    }
    match parameter {
        P::TableTick => *parts_mut(instrument)?.2 = value,
        P::Eq => parts_mut(instrument)?.3.associated_eq = value,
        P::Detune => parts_mut(instrument)?.3.fine_tune = value,
        P::Filter => parts_mut(instrument)?.3.filter_type = value,
        P::Cutoff => parts_mut(instrument)?.3.filter_cutoff = value,
        P::Resonance => parts_mut(instrument)?.3.filter_res = value,
        P::Amp => parts_mut(instrument)?.3.amp = value,
        P::Limit => {
            parts_mut(instrument)?.3.limit =
                LimitType::try_from(value).map_err(|_| EditError::InvalidValue)?
        }
        P::Pan => parts_mut(instrument)?.3.mixer_pan = value,
        P::Dry => parts_mut(instrument)?.3.mixer_dry = value,
        P::ModFx => parts_mut(instrument)?.3.mixer_mfx = value,
        P::Delay => parts_mut(instrument)?.3.mixer_delay = value,
        P::Reverb => parts_mut(instrument)?.3.mixer_reverb = value,
        P::ModType(slot) | P::ModDest(slot) | P::ModParam(slot, _) => {
            let m = parts_mut(instrument)?
                .3
                .mods
                .get_mut(usize::from(slot))
                .ok_or(EditError::OutOfRange)?;
            let (kind, mut bytes) = m8view::mod_bytes(m);
            match parameter {
                // A new type starts from the M8's defaults and keeps DEST.
                P::ModType(_) if value != kind => {
                    let dest = bytes[0] & 0x0F;
                    bytes[0] = value << 4 | dest;
                    bytes[1..].copy_from_slice(&MOD_DEFAULTS[usize::from(value)]);
                }
                P::ModType(_) => {}
                P::ModDest(_) => bytes[0] = (bytes[0] & 0xF0) | value,
                P::ModParam(_, k) if k < 5 => bytes[1 + usize::from(k)] = value,
                _ => return Err(EditError::OutOfRange),
            }
            *m = m8fmt::Mod::from_reader(&mut m8fmt::reader::Reader::new(bytes.to_vec()))
                .map_err(|_| EditError::InvalidValue)?;
        }
        _ => match instrument {
            Instrument::Sampler(s) => match parameter {
                P::Slice => s.slice = value,
                P::Play => {
                    s.play_mode =
                        SamplePlayMode::try_from(value).map_err(|_| EditError::InvalidValue)?
                }
                P::Start => s.start = value,
                P::LoopStart => s.loop_start = value,
                P::Length => s.length = value,
                P::Degrade => s.degrade = value,
                _ => return Err(EditError::UnsupportedOperation),
            },
            Instrument::FMSynth(s) => match parameter {
                P::FmAlgorithm => s.algo = FmAlgo(value),
                P::FmMod(0) => s.mod1 = value,
                P::FmMod(1) => s.mod2 = value,
                P::FmMod(2) => s.mod3 = value,
                P::FmMod(3) => s.mod4 = value,
                _ => {
                    let (id, kind) =
                        operator_parameter(parameter).ok_or(EditError::UnsupportedOperation)?;
                    let op = s
                        .operators
                        .get_mut(usize::from(id))
                        .ok_or(EditError::OutOfRange)?;
                    match kind {
                        OpKind::Wave => {
                            op.shape =
                                FMWave::try_from(value).map_err(|_| EditError::InvalidValue)?
                        }
                        OpKind::Ratio => op.ratio = value,
                        OpKind::RatioFine => op.ratio_fine = value,
                        OpKind::Level => op.level = value,
                        OpKind::Feedback => op.feedback = value,
                        OpKind::ModA => op.mod_a = value,
                        OpKind::ModB => op.mod_b = value,
                    }
                }
            },
            Instrument::MacroSynth(s) => match parameter {
                P::MacroShape => {
                    s.shape = MacroSynthOsc::try_from(value).map_err(|_| EditError::InvalidValue)?
                }
                P::MacroTimbre => s.timbre = value,
                P::MacroColor => s.color = value,
                P::Degrade => s.degrade = value,
                P::MacroRedux => s.redux = value,
                _ => return Err(EditError::UnsupportedOperation),
            },
            Instrument::WavSynth(s) => match parameter {
                P::WavShape => {
                    s.shape = WavShape::try_from(value).map_err(|_| EditError::InvalidValue)?
                }
                P::WavSize => s.size = value,
                P::WavMult => s.mult = value,
                P::WavWarp => s.warp = value,
                P::WavScan => s.scan = value,
                _ => return Err(EditError::UnsupportedOperation),
            },
            Instrument::HyperSynth(s) => match parameter {
                P::HyperScale => s.scale = value,
                P::HyperChord => {
                    // The M8 keeps the selected bank's intervals after the
                    // bank number (read back from a song it saved).
                    s.default_chord[0] = value;
                    s.default_chord[1..].copy_from_slice(&s.chords[usize::from(value)].offsets);
                }
                P::HyperInterval(k) => {
                    let bank = usize::from(s.default_chord[0] & 0x0F);
                    let k = usize::from(k);
                    if k >= 6 {
                        return Err(EditError::OutOfRange);
                    }
                    let chord = &mut s.chords[bank];
                    if value == INTERVAL_OFF {
                        chord.mask &= !(1 << k);
                        chord.offsets[k] = 0;
                    } else {
                        chord.mask |= 1 << k;
                        chord.offsets[k] = value;
                    }
                    s.default_chord[1 + k] = chord.offsets[k];
                }
                P::HyperShape => {
                    if s.shape.is_none() {
                        return Err(EditError::UnsupportedOperation);
                    }
                    s.shape = Some(
                        HyperSynthShape::try_from(value).map_err(|_| EditError::InvalidValue)?,
                    );
                }
                P::HyperShift => s.shift = value,
                P::HyperSwarm => s.swarm = value,
                P::HyperWidth => s.width = value,
                P::HyperSubosc => s.subosc = value,
                _ => return Err(EditError::UnsupportedOperation),
            },
            _ => return Err(EditError::UnsupportedOperation),
        },
    }
    Ok(())
}

/// Bytes 1-5 of a modulator when its type is set, as the target device shows
/// them (15/10/2026): AHD, ADSR, DRUM, LFO, TRIG ENV, TRACKING.
const MOD_DEFAULTS: [[u8; 5]; 6] = [
    [0xFF, 0x00, 0x00, 0x80, 0xFF],
    [0xFF, 0x00, 0x80, 0x80, 0x80],
    [0xFF, 0x80, 0x10, 0x80, 0xFF],
    [0xFF, 0x00, 0x00, 0x10, 0xFF],
    [0xFF, 0x00, 0x00, 0x40, 0x00],
    [0xFF, 0x00, 0x00, 0x7F, 0xFF],
];

/// The M8's value for a modulator parameter of the type it has now.
pub(crate) fn mod_param_default(song: &Song, field: Field) -> Option<u8> {
    let Field::Instrument {
        index,
        parameter: P::ModParam(slot, k),
    } = field
    else {
        return None;
    };
    let params = m8view::mod_params(song.instruments.get(usize::from(index))?)?;
    let (kind, _) = m8view::mod_bytes(params.mods.get(usize::from(slot))?);
    MOD_DEFAULTS
        .get(usize::from(kind))?
        .get(usize::from(k))
        .copied()
}

/// An interval whose oscillator is off ("--" on the CHORD line).
pub(crate) const INTERVAL_OFF: u8 = 0xFF;

pub(crate) fn byte_range(
    instrument: &Instrument,
    parameter: P,
) -> Result<(u8, u8, u8, u8), EditError> {
    let (min, max, fine, coarse) = match parameter {
        P::Name | P::SamplePath | P::Transpose => return Err(EditError::UnsupportedOperation),
        P::Eq => (0, 0x80, 1, 16),
        P::Play => (0, 14, 1, 1),
        // WAVSYNTH adds WAV LP, HP, BP and BS.
        P::Filter if matches!(instrument, Instrument::WavSynth(_)) => (0, 11, 1, 1),
        P::Filter => (0, 7, 1, 1),
        P::Limit => (0, 8, 1, 1),
        P::FmAlgorithm => (0, 11, 1, 1),
        P::FmWave(_) => (0, 0x45, 1, 1),
        P::FmRatio(_) | P::FmRatioFine(_) => (0, 99, 1, 10),
        // 1-4 LEV, 5-8 RAT, 9-12 PIT, 13-16 FBK.
        P::FmModA(_) | P::FmModB(_) => (0, 16, 1, 1),
        P::MacroShape => (0, 46, 1, 1),
        P::WavShape => (0, 69, 1, 1),
        P::HyperChord => (0, 15, 1, 1),
        P::HyperInterval(_) => (0, INTERVAL_OFF - 1, 1, 12),
        P::HyperShape => (0, 11, 1, 1),
        P::ModType(_) => (0, 5, 1, 1),
        P::ModDest(_) => (
            0,
            (m8view::mod_destinations(instrument).len().max(1) - 1) as u8,
            1,
            1,
        ),
        P::ModParam(slot, k) => {
            let kind = m8view::mod_params(instrument)
                .and_then(|p| p.mods.get(usize::from(slot)))
                .map_or(0, |m| m8view::mod_bytes(m).0);
            match (kind, k) {
                (3, 1) => (0, 19, 1, 1),    // LFO OSC
                (3, 2) => (0, 3, 1, 1),     // LFO TRIG
                (5, 1) => (0, 2, 1, 1),     // TRACKING SRC
                (4, 4) => (0, 0x87, 1, 16), // TRIG ENV SRC: instruments, tracks
                _ => (0, 255, 1, 16),
            }
        }
        _ => (0, 255, 1, 16),
    };
    Ok((min, max, fine, coarse))
}

pub(crate) fn clear_value(field: Field) -> Result<FieldValue, EditError> {
    let Field::Instrument { parameter, .. } = field else {
        return Err(EditError::UnsupportedOperation);
    };
    Ok(match parameter {
        P::Name | P::SamplePath => FieldValue::Text(String::new()),
        P::Transpose => FieldValue::Bool(true),
        P::TableTick => FieldValue::Byte(1),
        P::Eq => FieldValue::Byte(0x80),
        P::Detune | P::Pan => FieldValue::Byte(0x80),
        P::Length | P::Cutoff => FieldValue::Byte(0xFF),
        P::WavSize => FieldValue::Byte(0x20),
        P::MacroTimbre | P::MacroColor | P::HyperShift | P::HyperSubosc => FieldValue::Byte(0x80),
        P::Dry => FieldValue::Byte(0xC0),
        P::FmRatio(_) => FieldValue::Byte(1),
        _ => FieldValue::Byte(0),
    })
}

pub(crate) fn default_insert_value(field: Field) -> Result<FieldValue, EditError> {
    clear_value(field)
}
