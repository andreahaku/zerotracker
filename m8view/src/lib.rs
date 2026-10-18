//! Read-only, cell-based views of a parsed song.
//!
//! Frames contain the stable left-hand data panel. Playback meters, live
//! transport data, and the cursor belong to the UI and are deliberately absent.

use m8fmt::{CommandPack, FX, Instrument, SamplePlayMode, Song};

mod fields;
pub use fields::{field_spans, fx_commands};

/// A view that can be opened without changing the song.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Song { top_row: u8 },
    Chain { index: u8 },
    Phrase { index: u8 },
    Instrument { index: u8 },
    Table { index: u8 },
}

/// A writable value in a song, independent of its current screen position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Field {
    SongName,
    SongTempo,
    SongTranspose,
    MixerTrackVolume {
        track: u8,
    },
    MixerMasterVolume,
    MixerLimiter,
    SongChain {
        row: u8,
        track: u8,
    },
    ChainPhrase {
        chain: u8,
        row: u8,
    },
    ChainTranspose {
        chain: u8,
        row: u8,
    },
    PhraseNote {
        phrase: u8,
        row: u8,
    },
    PhraseVelocity {
        phrase: u8,
        row: u8,
    },
    PhraseInstrument {
        phrase: u8,
        row: u8,
    },
    PhraseFxCommand {
        phrase: u8,
        row: u8,
        lane: u8,
    },
    PhraseFxValue {
        phrase: u8,
        row: u8,
        lane: u8,
    },
    TableTranspose {
        table: u8,
        row: u8,
    },
    TableVelocity {
        table: u8,
        row: u8,
    },
    TableFxCommand {
        table: u8,
        row: u8,
        lane: u8,
    },
    TableFxValue {
        table: u8,
        row: u8,
        lane: u8,
    },
    Instrument {
        index: u8,
        parameter: InstrumentParameter,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InstrumentParameter {
    Kind,
    Name,
    Transpose,
    TableTick,
    Eq,
    SamplePath,
    SampleEdit,
    Load,
    Save,
    Slice,
    Play,
    Start,
    LoopStart,
    Length,
    Detune,
    Degrade,
    Filter,
    Cutoff,
    Resonance,
    Amp,
    Limit,
    Pan,
    Dry,
    ModFx,
    Delay,
    Reverb,
    FmAlgorithm,
    FmWave(u8),
    FmRatio(u8),
    FmRatioFine(u8),
    FmLevel(u8),
    FmFeedback(u8),
    FmModA(u8),
    FmModB(u8),
    FmMod(u8),
    MacroShape,
    MacroTimbre,
    MacroColor,
    MacroRedux,
    WavShape,
    WavSize,
    WavMult,
    WavWarp,
    WavScan,
    HyperScale,
    /// The chord bank (00-0F) on the CHORD line.
    HyperChord,
    /// One of the bank's six intervals; 0xFF when the oscillator is off.
    HyperInterval(u8),
    HyperShape,
    HyperShift,
    HyperSwarm,
    HyperWidth,
    HyperSubosc,
    /// Modulator 0-3: its type (AHD ENV ... TRACKING).
    ModType(u8),
    /// Modulator 0-3: its destination.
    ModDest(u8),
    /// Modulator 0-3, parameter 0-4 (byte 1-5 of the modulator: AMT, then
    /// the type's own, as on the M8's MODS screen).
    ModParam(u8, u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldSpan {
    pub field: Field,
    pub line: usize,
    pub column: usize,
    pub width: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellRole {
    Label,
    Value,
    Empty,
    Effect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub glyph: char,
    pub role: CellRole,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Line {
    pub cells: Vec<Cell>,
}

impl Line {
    fn push(&mut self, text: &str, role: CellRole) {
        self.cells
            .extend(text.chars().map(|glyph| Cell { glyph, role }));
    }

    pub fn text(&self) -> String {
        self.cells.iter().map(|cell| cell.glyph).collect()
    }

    fn put(&mut self, column: usize, text: &str, role: CellRole) {
        while self.cells.len() < column {
            self.push(" ", CellRole::Label);
        }
        for (offset, glyph) in text.chars().enumerate() {
            let cell = Cell { glyph, role };
            if column + offset < self.cells.len() {
                self.cells[column + offset] = cell;
            } else {
                self.cells.push(cell);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub lines: Vec<Line>,
}

impl Frame {
    pub fn plain_lines(&self) -> Vec<String> {
        self.lines.iter().map(Line::text).collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewError {
    IndexOutOfRange { view: &'static str, index: u8 },
    UnsupportedInstrumentType { index: u8 },
}

/// Render one stable data panel. All rows use the M8 text-screen layout.
pub fn render(song: &Song, view: View) -> Result<Frame, ViewError> {
    match view {
        View::Song { top_row } => Ok(song_view(song, top_row)),
        View::Chain { index } => song
            .chains
            .get(usize::from(index))
            .map(|_| chain_view(song, index))
            .ok_or(ViewError::IndexOutOfRange {
                view: "chain",
                index,
            }),
        View::Phrase { index } => song
            .phrases
            .get(usize::from(index))
            .map(|_| phrase_view(song, index))
            .ok_or(ViewError::IndexOutOfRange {
                view: "phrase",
                index,
            }),
        View::Instrument { index } => song
            .instruments
            .get(usize::from(index))
            .ok_or(ViewError::IndexOutOfRange {
                view: "instrument",
                index,
            })
            .and_then(|_| instrument_view(song, index)),
        View::Table { index } => song
            .tables
            .get(usize::from(index))
            .map(|_| table_view(song, index))
            .ok_or(ViewError::IndexOutOfRange {
                view: "table",
                index,
            }),
    }
}

fn title(name: &str, index: Option<u8>) -> Line {
    let mut line = Line::default();
    line.push(" ", CellRole::Label);
    line.push(name, CellRole::Label);
    if let Some(index) = index {
        line.push(&format!(" {index:02X}"), CellRole::Value);
    }
    line
}

fn song_view(song: &Song, top_row: u8) -> Frame {
    let mut lines = vec![title("SONG", None)];
    let mut header = Line::default();
    header.push("    1  2  3  4  5  6  7  8", CellRole::Label);
    lines.push(header);
    let start = usize::from(top_row).min(240);
    for row in start..start + 16 {
        let mut line = Line::default();
        line.push(&format!(" {row:02X}"), CellRole::Label);
        for track in 0..8 {
            let value = song.song.steps[row * 8 + track];
            line.push(" ", CellRole::Label);
            if value == 0xFF {
                line.push("--", CellRole::Empty);
            } else {
                line.push(&format!("{value:02X}"), CellRole::Value);
            }
        }
        lines.push(line);
    }
    Frame { lines }
}

fn chain_view(song: &Song, index: u8) -> Frame {
    let chain = &song.chains[usize::from(index)];
    let mut lines = vec![title("CHAIN", Some(index))];
    let mut header = Line::default();
    header.push("   PH TSP", CellRole::Label);
    lines.push(header);
    for (row, step) in chain.steps.iter().enumerate() {
        let mut line = Line::default();
        line.push(&format!(" {row:X} "), CellRole::Label);
        if step.phrase == 0xFF {
            line.push("--", CellRole::Empty);
        } else {
            line.push(&format!("{:02X}", step.phrase), CellRole::Value);
        }
        line.push(" ", CellRole::Label);
        line.push(&format!("{:02X}", step.transpose), CellRole::Value);
        lines.push(line);
    }
    Frame { lines }
}

fn phrase_view(song: &Song, index: u8) -> Frame {
    let phrase = &song.phrases[usize::from(index)];
    let mut lines = vec![title("PHRASE", Some(index))];
    let mut header = Line::default();
    header.push("   N   V  I  FX1   FX2   FX3", CellRole::Label);
    lines.push(header);
    let mut command_pack = CommandPack::default();
    for (row, step) in phrase.steps.iter().enumerate() {
        if let Some(instrument) = song.instruments.get(usize::from(step.instrument)) {
            command_pack = instrument.instr_command_text(song.version);
        }
        let mut line = Line::default();
        line.push(&format!(" {row:X} "), CellRole::Label);
        let note = step.note.to_string();
        line.push(
            &note,
            if step.note.is_empty() {
                CellRole::Empty
            } else {
                CellRole::Value
            },
        );
        line.push(" ", CellRole::Label);
        value_or_empty(&mut line, step.velocity);
        line.push(" ", CellRole::Label);
        value_or_empty(&mut line, step.instrument);
        for fx in [step.fx1, step.fx2, step.fx3] {
            line.push(" ", CellRole::Label);
            line.push(&effect(song, fx, command_pack), CellRole::Effect);
        }
        lines.push(line);
    }
    Frame { lines }
}

fn value_or_empty(line: &mut Line, value: u8) {
    if value == 0xFF {
        line.push("--", CellRole::Empty);
    } else {
        line.push(&format!("{value:02X}"), CellRole::Value);
    }
}

fn effect(song: &Song, fx: FX, pack: CommandPack) -> String {
    let command = if fx.command == 0xFF {
        "---".to_owned()
    } else if let Some(name) = FX::fx_command_names(song.version).try_render(fx.command) {
        name.to_owned()
    } else if let Some(name) = pack.try_render(fx.command) {
        name.to_owned()
    } else {
        format!("?{:02X}", fx.command)
    };
    format!("{command}{:02X}", fx.value)
}

fn instrument_view(song: &Song, index: u8) -> Result<Frame, ViewError> {
    let mut frame = instrument_page(song, index)?;
    if let Some(params) = mod_params(&song.instruments[usize::from(index)]) {
        append_mods(
            song,
            &song.instruments[usize::from(index)],
            params,
            &mut frame.lines,
        );
    }
    Ok(frame)
}

fn instrument_page(song: &Song, index: u8) -> Result<Frame, ViewError> {
    match &song.instruments[usize::from(index)] {
        Instrument::Sampler(sampler) => Ok(sampler_view(song, index, sampler)),
        Instrument::FMSynth(fm) => Ok(fm_view(index, fm)),
        Instrument::MacroSynth(macro_synth) => Ok(macro_view(index, macro_synth)),
        Instrument::WavSynth(wav) => Ok(wav_view(index, wav)),
        Instrument::HyperSynth(hyper) => Ok(hyper_view(song, index, hyper)),
        Instrument::None => Ok(none_view(index)),
        Instrument::MIDIOut(midi) => Ok(unplayed_view(
            index,
            "MIDI OUT",
            &midi.name,
            (midi.transpose, midi.table_tick),
        )),
        Instrument::External(external) => Ok(unplayed_view(
            index,
            "EXTERNAL",
            &external.name,
            (external.transpose, external.table_tick),
        )),
    }
}

fn instrument_intro(
    index: u8,
    kind: &str,
    name: &str,
    settings: Option<(bool, u8, u8)>,
) -> Vec<Line> {
    let mut lines = vec![title("INST.", Some(index))];
    let mut line = Line::default();
    line.put(1, "TYPE", CellRole::Label);
    line.put(9, kind, CellRole::Value);
    line.put(23, "LOAD", CellRole::Label);
    line.put(28, "SAVE", CellRole::Label);
    lines.push(line);

    let mut line = Line::default();
    line.put(1, "NAME", CellRole::Label);
    let mut name = name.chars().take(12).collect::<String>();
    while name.chars().count() < 12 {
        name.push('-');
    }
    line.put(
        9,
        &name.chars().take(8).collect::<String>(),
        CellRole::Value,
    );
    line.put(
        18,
        &name.chars().skip(8).collect::<String>(),
        CellRole::Value,
    );
    lines.push(line);

    if let Some((transpose, table_tick, eq)) = settings {
        let mut line = Line::default();
        line.put(1, "TRANSP.", CellRole::Label);
        line.put(9, if transpose { "ON" } else { "OFF" }, CellRole::Value);
        line.put(14, "TBL.TIC", CellRole::Label);
        line.put(22, &format!("{table_tick:02X}"), CellRole::Value);
        line.put(27, "EQ", CellRole::Label);
        line.put(
            30,
            &if eq >= 0x80 {
                "--".to_owned()
            } else {
                format!("{eq:02X}")
            },
            CellRole::Value,
        );
        lines.push(line);
        lines.push(Line::default());
    }
    lines
}

fn sampler_view(song: &Song, index: u8, sampler: &m8fmt::Sampler) -> Frame {
    let mut lines = instrument_intro(
        index,
        "SAMPLER",
        &sampler.name,
        Some((
            sampler.transpose,
            sampler.table_tick,
            sampler.synth_params.associated_eq,
        )),
    );

    let mut line = Line::default();
    line.put(1, "SAMPLE", CellRole::Label);
    let sample_name = std::path::Path::new(&sampler.sample_path)
        .file_stem()
        .map(|name| name.to_string_lossy().to_ascii_uppercase())
        .unwrap_or_default();
    line.put(
        9,
        &sample_name.chars().take(16).collect::<String>(),
        CellRole::Value,
    );
    // No sample yet: REC. records one (the M8's SAMPLE LOAD / REC.).
    line.put(
        26,
        if sampler.sample_path.is_empty() {
            "REC."
        } else {
            "EDIT"
        },
        CellRole::Label,
    );
    lines.push(line);
    lines.push(Line::default());

    let mut line = Line::default();
    line.put(1, "SLICE", CellRole::Label);
    line.put(9, &slice_label(sampler.slice), CellRole::Value);
    line.put(19, "AMP", CellRole::Label);
    line.put(
        23,
        &format!("{:02X}", sampler.synth_params.amp),
        CellRole::Value,
    );
    lines.push(line);

    let mut line = Line::default();
    line.put(1, "PLAY", CellRole::Label);
    let play_mode: u8 = sampler.play_mode.into();
    let play_name = match sampler.play_mode {
        SamplePlayMode::FWD_PP => "FWD PP".to_owned(),
        other => format!("{other:?}").replace('_', "."),
    };
    line.put(9, &format!("{play_mode:02X}{play_name}"), CellRole::Value);
    line.put(19, "LIM", CellRole::Label);
    line.put(
        23,
        &limit_label(sampler.synth_params.limit),
        CellRole::Value,
    );
    lines.push(line);

    let bpm_mode = matches!(
        sampler.play_mode,
        SamplePlayMode::REP_BPM | SamplePlayMode::BPM_REV | SamplePlayMode::BPM_PP
    );
    let sampler_rows = if bpm_mode {
        [
            ("BPM", sampler.synth_params.fine_tune),
            ("START", sampler.start),
            ("LOOP ST", sampler.loop_start),
            ("LENGTH", sampler.length),
            ("DEGRADE", sampler.degrade),
        ]
    } else {
        [
            ("START", sampler.start),
            ("LOOP ST", sampler.loop_start),
            ("LENGTH", sampler.length),
            ("DETUNE", sampler.synth_params.fine_tune),
            ("DEGRADE", sampler.degrade),
        ]
    };
    let mixer_rows = [
        ("PAN", sampler.synth_params.mixer_pan),
        ("DRY", sampler.synth_params.mixer_dry),
        ("MFX", sampler.synth_params.mixer_mfx),
        ("DEL", sampler.synth_params.mixer_delay),
        ("REV", sampler.synth_params.mixer_reverb),
    ];
    for ((label, value), (right_label, right_value)) in sampler_rows.into_iter().zip(mixer_rows) {
        let mut line = Line::default();
        line.put(1, label, CellRole::Label);
        line.put(9, &format!("{value:02X}"), CellRole::Value);
        line.put(19, right_label, CellRole::Label);
        line.put(23, &format!("{right_value:02X}"), CellRole::Value);
        lines.push(line);
    }

    let mut line = Line::default();
    line.put(1, "FILTER", CellRole::Label);
    let filter_name = sampler
        .filter_types(song.version)
        .get(usize::from(sampler.synth_params.filter_type))
        .copied()
        .unwrap_or("???");
    line.put(
        9,
        &format!("{:02X}{filter_name}", sampler.synth_params.filter_type),
        CellRole::Value,
    );
    lines.push(line);
    for (label, value) in [
        ("CUTOFF", sampler.synth_params.filter_cutoff),
        ("RES", sampler.synth_params.filter_res),
    ] {
        let mut line = Line::default();
        line.put(1, label, CellRole::Label);
        line.put(9, &format!("{value:02X}"), CellRole::Value);
        lines.push(line);
    }
    lines.push(Line::default());
    Frame { lines }
}

/// SLICE as the M8 shows it: OFF, FILE (the WAV's markers), or the number
/// of equal slices with its decimal value.
fn slice_label(value: u8) -> String {
    match value {
        0 => "00OFF".to_owned(),
        1 => "01FILE".to_owned(),
        n => format!("{n:02X}({n:03})"),
    }
}

/// A FILTER value with its name ("01LOWPASS").
fn filter_label(types: &[&str], value: u8) -> String {
    let name = types.get(usize::from(value)).copied().unwrap_or("???");
    format!("{value:02X}{name}")
}

/// A LIM value with its name ("00CLIP").
fn limit_label(limit: m8fmt::LimitType) -> String {
    // m8fmt names ids 00-08 and indexes without a check.
    let name = if limit.id() <= 8 { limit.str() } else { "???" };
    format!("{:02X}{name}", limit.id())
}

fn fm_mod_label(value: u8) -> String {
    match value {
        0 => "-----".to_owned(),
        1..=4 => format!("{value}>LEV"),
        5..=8 => format!("{}>RAT", value - 4),
        9..=12 => format!("{}>PIT", value - 8),
        13..=16 => format!("{}>FBK", value - 12),
        _ => format!("?{value:04X}"),
    }
}

fn fm_view(index: u8, fm: &m8fmt::FMSynth) -> Frame {
    let mut lines = instrument_intro(
        index,
        "FMSYNTH",
        &fm.name,
        Some((fm.transpose, fm.table_tick, fm.synth_params.associated_eq)),
    );
    let mut line = Line::default();
    line.put(1, "ALGO", CellRole::Label);
    line.put(
        9,
        &format!("{:02X}{}", fm.algo.id(), fm.algo.str()),
        CellRole::Value,
    );
    lines.push(line);

    let mut line = Line::default();
    for (operator, op) in fm.operators.iter().enumerate() {
        line.put(
            9 + operator * 6,
            &format!("{} {:?}", (b'A' + operator as u8) as char, op.shape),
            CellRole::Value,
        );
    }
    lines.push(line);

    let mut line = Line::default();
    line.put(1, "RATIO", CellRole::Label);
    for (operator, op) in fm.operators.iter().enumerate() {
        line.put(
            9 + operator * 6,
            &format!("{:02}.{:02}", op.ratio, op.ratio_fine),
            CellRole::Value,
        );
    }
    lines.push(line);

    let mut line = Line::default();
    line.put(1, "LEV/FB", CellRole::Label);
    for (operator, op) in fm.operators.iter().enumerate() {
        line.put(
            9 + operator * 6,
            &format!("{:02X}/{:02X}", op.level, op.feedback),
            CellRole::Value,
        );
    }
    lines.push(line);

    for (label, field) in [("MOD", true), ("", false)] {
        let mut line = Line::default();
        if !label.is_empty() {
            line.put(1, label, CellRole::Label);
        }
        for (operator, op) in fm.operators.iter().enumerate() {
            let value = if field { op.mod_a } else { op.mod_b };
            line.put(9 + operator * 6, &fm_mod_label(value), CellRole::Value);
        }
        lines.push(line);
    }

    let p = &fm.synth_params;
    let filter = filter_label(fm.filter_types(m8fmt::Version::default()), p.filter_type);
    let left_rows = [
        ("MOD1", hex(fm.mod1)),
        ("MOD2", hex(fm.mod2)),
        ("MOD3", hex(fm.mod3)),
        ("MOD4", hex(fm.mod4)),
        ("FILTER", filter),
        ("CUTOFF", hex(p.filter_cutoff)),
        ("RES", hex(p.filter_res)),
    ];
    Frame {
        lines: with_mixer_rows(lines, left_rows, p),
    }
}

fn macro_view(index: u8, macro_synth: &m8fmt::MacroSynth) -> Frame {
    let mut lines = instrument_intro(
        index,
        "MACROSYN",
        &macro_synth.name,
        Some((
            macro_synth.transpose,
            macro_synth.table_tick,
            macro_synth.synth_params.associated_eq,
        )),
    );
    let mut line = Line::default();
    line.put(1, "SHAPE", CellRole::Label);
    let shape: u8 = macro_synth.shape.into();
    line.put(
        9,
        &format!("{shape:02X}{:?}", macro_synth.shape).replace('_', " "),
        CellRole::Value,
    );
    lines.push(line);
    lines.push(Line::default());

    let p = &macro_synth.synth_params;
    let filter = filter_label(
        macro_synth.filter_types(m8fmt::Version::default()),
        p.filter_type,
    );
    let left_rows = [
        ("TIMBRE", hex(macro_synth.timbre)),
        ("COLOR", hex(macro_synth.color)),
        ("DEGRADE", hex(macro_synth.degrade)),
        ("REDUX", hex(macro_synth.redux)),
        ("FILTER", filter),
        ("CUTOFF", hex(p.filter_cutoff)),
        ("RES", hex(p.filter_res)),
    ];
    let mut lines = with_mixer_rows(lines, left_rows, p);
    lines.resize_with(17, Line::default);
    Frame { lines }
}

fn wav_view(index: u8, wav: &m8fmt::WavSynth) -> Frame {
    let mut lines = instrument_intro(
        index,
        "WAVSYNTH",
        &wav.name,
        Some((
            wav.transpose,
            wav.table_tick,
            wav.synth_params.associated_eq,
        )),
    );
    let mut line = Line::default();
    line.put(1, "SHAPE", CellRole::Label);
    line.put(9, &wav_shape_label(wav.shape.into()), CellRole::Value);
    lines.push(line);
    lines.push(Line::default());
    let p = &wav.synth_params;
    let filter = filter_label(wav.filter_types(m8fmt::Version::default()), p.filter_type);
    let left_rows = [
        ("SIZE", hex(wav.size)),
        ("MULT", hex(wav.mult)),
        ("WARP", hex(wav.warp)),
        ("SCAN", hex(wav.scan)),
        ("FILTER", filter),
        ("CUTOFF", hex(p.filter_cutoff)),
        ("RES", hex(p.filter_res)),
    ];
    let mut lines = with_mixer_rows(lines, left_rows, p);
    lines.resize_with(17, Line::default);
    Frame { lines }
}

/// A WAVSYNTH SHAPE value with its name ("09WT-OSC:CRUSH").
fn wav_shape_label(value: u8) -> String {
    let name = WAV_SHAPE_NAMES
        .get(usize::from(value))
        .copied()
        .unwrap_or("???");
    format!("{value:02X}{name}")
}

/// Read on the target device (15/10/2026).
const WAV_SHAPE_NAMES: [&str; 70] = [
    "PULSE 12%",
    "PULSE 25%",
    "PULSE 50%",
    "PULSE 75%",
    "SAW",
    "TRIANGLE",
    "SINE",
    "NOISE PITCHED",
    "NOISE",
    "WT-OSC:CRUSH",
    "WT-OSC:FOLDING",
    "WT-OSC:FREQ",
    "WT-OSC:FUZZY",
    "WT-OSC:GHOST",
    "WT-OSC:GRAPHIC",
    "WT-OSC:LFOPLAY",
    "WT-OSC:LIQUID",
    "WT-OSC:MORPHING",
    "WT-OSC:MYSTIC",
    "WT-OSC:STICKY",
    "WT-OSC:TIDAL",
    "WT-OSC:TIDY",
    "WT-OSC:TUBE",
    "WT-OSC:UMBRELLA",
    "WT-OSC:UNWIND",
    "WT-OSC:VIRAL",
    "WT-OSC:WAVES",
    "WT-BNK:DRIP",
    "WT-BNK:FROGGY",
    "WT-BNK:INSONIC",
    "WT-BNK:RADIUS",
    "WT-BNK:SCRATCH",
    "WT-BNK:SMOOTH",
    "WT-BNK:WOBBLE",
    "WT-HRM:ASYMMTRY",
    "WT-HRM:BLEEN",
    "WT-HRM:FRACTAL",
    "WT-HRM:GENTLE",
    "WT-HRM:HARMONIC",
    "WT-HRM:HYPNOTIC",
    "WT-HRM:ITERATIV",
    "WT-HRM:MICROWAV",
    "WT-HRM:PLAITS01",
    "WT-HRM:PLAITS02",
    "WT-HRM:RISEFALL",
    "WT-HRM:TONAL",
    "WT-HRM:TWINE",
    "WT-EFX:ALIEN",
    "WT-EFX:CYBERNET",
    "WT-EFX:DISORDR",
    "WT-EFX:FORMANT",
    "WT-EFX:HYPER",
    "WT-EFX:JAGGED",
    "WT-EFX:MIXED",
    "WT-EFX:MULTIPLY",
    "WT-EFX:NOWHERE",
    "WT-EFX:PINBALL",
    "WT-EFX:RINGS",
    "WT-EFX:SHIMMER",
    "WT-EFX:SPECTRAL",
    "WT-EFX:SPOOKY",
    "WT-EFX:TRANSFRM",
    "WT-EFX:TWISTED",
    "WT-EFX:VOCAL",
    "WT-EFX:WASHED",
    "WT-EFX:WONDER",
    "WT-EFX:WOWEE",
    "WT-EFX:ZAP",
    "WT-VOX:BRAIDS",
    "WT-VOX:VOXSYNTH",
];

/// HYPERSYNTH shapes (firmware 6.6 and later).
const HYPER_SHAPE_NAMES: [&str; 12] = [
    "SAW",
    "SOFT SAW",
    "DARK SAW",
    "SQUARE SOFT",
    "TRIANGLE",
    "SINE",
    "SINE 3X",
    "SINE FB",
    "SINE FOLD",
    "SINE RING",
    "SINE HALF",
    "SINE ORGAN",
];

const NOTE_NAMES: [&str; 12] = [
    "C ", "C#", "D ", "D#", "E ", "F ", "F#", "G ", "G#", "A ", "A#", "B ",
];

/// SCALE on HYPERSYNTH: 00 follows the song, else the high digit is the
/// root (B from B0 up) and the low digit one of the song's 16 scales.
fn hyper_scale_label(song: &Song, value: u8) -> String {
    if value == 0 {
        return "00DEFAULT".to_owned();
    }
    let root = NOTE_NAMES[usize::from(value >> 4).min(11)];
    let name = song
        .scales
        .get(usize::from(value & 0x0F))
        .map_or("", |scale| scale.name.as_str());
    format!("{value:02X}{root}{name}")
}

fn hyper_view(song: &Song, index: u8, hyper: &m8fmt::HyperSynth) -> Frame {
    let mut lines = instrument_intro(
        index,
        "HYPERSYN",
        &hyper.name,
        Some((
            hyper.transpose,
            hyper.table_tick,
            hyper.synth_params.associated_eq,
        )),
    );
    let mut line = Line::default();
    line.put(1, "SCALE", CellRole::Label);
    line.put(9, &hyper_scale_label(song, hyper.scale), CellRole::Value);
    lines.push(line);

    let mut line = Line::default();
    line.put(1, "CHORD", CellRole::Label);
    let bank = hyper.default_chord[0] & 0x0F;
    line.put(9, &hex(bank), CellRole::Value);
    line.put(11, ">", CellRole::Label);
    let chord = &hyper.chords[usize::from(bank)];
    for k in 0..6 {
        line.put(13 + 3 * k, &chord.offset_str(k), CellRole::Value);
    }
    lines.push(line);
    lines.push(Line::default());
    lines.push(Line::default());

    let mut line = Line::default();
    if let Some(shape) = hyper.shape {
        let id: u8 = shape.into();
        line.put(1, "SHAPE", CellRole::Label);
        let name = HYPER_SHAPE_NAMES
            .get(usize::from(id))
            .copied()
            .unwrap_or("???");
        line.put(9, &format!("{id:02X}{name}"), CellRole::Value);
    }
    lines.push(line);

    let p = &hyper.synth_params;
    let filter = filter_label(hyper.filter_types(m8fmt::Version::default()), p.filter_type);
    let left_rows = [
        ("SHIFT", hex(hyper.shift)),
        ("SWARM", hex(hyper.swarm)),
        ("WIDTH", hex(hyper.width)),
        ("SUBOSC", hex(hyper.subosc)),
        ("FILTER", filter),
        ("CUTOFF", hex(p.filter_cutoff)),
        ("RES", hex(p.filter_res)),
    ];
    Frame {
        lines: with_mixer_rows(lines, left_rows, p),
    }
}

/// First modulator line, under the sound types' fields (FMSYNTH and
/// SAMPLER use 18 lines) and the MODS header.
pub const MODS_LINE: usize = 19;
/// Value column of MOD3 and MOD4: their labels have four letters (DEST,
/// TRIG, FREQ), one more than AMP ... REV above.
pub const MODS_RIGHT_VALUE: usize = 24;
/// Lines from the section's first modulator line to the second pair.
pub const MODS_PAIR_GAP: usize = 8;

pub const MOD_TYPE_NAMES: [&str; 6] = [
    "AHD ENV", "ADSR ENV", "DRUM ENV", "LFO", "TRIG ENV", "TRACKING",
];

/// Labels of a modulator's five parameter bytes, by type ("" = not shown).
pub const MOD_PARAM_LABELS: [[&str; 5]; 6] = [
    ["AMT", "ATK", "HOLD", "DEC", ""],
    ["AMT", "ATK", "DEC", "SUS", "REL"],
    ["AMT", "PEAK", "BODY", "DEC", ""],
    ["AMT", "OSC", "TRIG", "FREQ", ""],
    ["AMT", "ATK", "HOLD", "DEC", "SRC"],
    ["AMT", "SRC", "LVAL", "HVAL", ""],
];

/// LFO shapes and trigger modes, and TRACKING sources, as on the M8.
const LFO_SHAPE_NAMES: [&str; 20] = [
    "TRI", "SIN", "RAMP DN", "RAMP UP", "EXP DN", "EXP UP", "SQU DN", "SQU UP", "RANDOM", "DRUNK",
    "TRI T", "SIN T", "RAMPDN T", "RAMPUP T", "EXP DN T", "EXP UP T", "SQU DN T", "SQU UP T",
    "RAND T", "DRUNK T",
];
const LFO_TRIGGER_NAMES: [&str; 4] = ["FREE", "RETRIG", "HOLD", "ONCE"];
const TRACKING_SOURCE_NAMES: [&str; 3] = ["NOTE", "VELOCITY", "VEL.TAKE"];

/// The synth parameters (and so the modulators) of the sound types.
pub fn mod_params(instrument: &Instrument) -> Option<&m8fmt::SynthParams> {
    match instrument {
        Instrument::WavSynth(s) => Some(&s.synth_params),
        Instrument::MacroSynth(s) => Some(&s.synth_params),
        Instrument::Sampler(s) => Some(&s.synth_params),
        Instrument::FMSynth(s) => Some(&s.synth_params),
        Instrument::HyperSynth(s) => Some(&s.synth_params),
        _ => None,
    }
}

/// A modulator's type (0-5) and its six bytes as the file stores them.
pub fn mod_bytes(m: &m8fmt::Mod) -> (u8, [u8; 6]) {
    let mut w = m8fmt::writer::Writer::new(vec![0xFF; 6]);
    m.write(&mut w);
    let bytes: [u8; 6] = w.finish()[..6].try_into().expect("six bytes");
    (bytes[0] >> 4, bytes)
}

/// The DEST names of an instrument type.
pub fn mod_destinations(instrument: &Instrument) -> &'static [&'static str] {
    let v = m8fmt::Version::default();
    match instrument {
        Instrument::WavSynth(s) => s.destination_names(v),
        Instrument::MacroSynth(s) => s.destination_names(v),
        Instrument::Sampler(s) => s.destination_names(v),
        Instrument::FMSynth(s) => s.destination_names(v),
        Instrument::HyperSynth(s) => s.destination_names(v),
        _ => &[],
    }
}

fn kind_name(instrument: &Instrument) -> &'static str {
    match instrument {
        Instrument::WavSynth(_) => "WAVSYNTH",
        Instrument::MacroSynth(_) => "MACROSYN",
        Instrument::Sampler(_) => "SAMPLER",
        Instrument::MIDIOut(_) => "MIDI OUT",
        Instrument::FMSynth(_) => "FMSYNTH",
        Instrument::HyperSynth(_) => "HYPERSYN",
        Instrument::External(_) => "EXTERNAL",
        Instrument::None => "NONE",
    }
}

/// One parameter's text: names for the choices, else hex.
fn mod_param_text(song: &Song, kind: u8, k: usize, value: u8) -> String {
    let named = |names: &[&str]| {
        let name = names.get(usize::from(value)).copied().unwrap_or("???");
        format!("{value:02X}{name}")
    };
    match (kind, k) {
        (3, 1) => named(&LFO_SHAPE_NAMES),
        (3, 2) => named(&LFO_TRIGGER_NAMES),
        (5, 1) => named(&TRACKING_SOURCE_NAMES),
        // TRIG ENV source: an instrument (its name, or its type when it has
        // none), or all instruments on track 1-8 from 80.
        (4, 4) => {
            let name = if value >= 0x80 {
                format!("TRACK {}", value - 0x7F)
            } else {
                song.instruments
                    .get(usize::from(value))
                    .map(|i| match i.name().map(str::trim) {
                        Some(n) if !n.is_empty() => n.to_owned(),
                        _ => kind_name(i).to_owned(),
                    })
                    .unwrap_or_default()
            };
            format!("{value:02X}{}", name.chars().take(8).collect::<String>())
        }
        _ => hex(value),
    }
}

/// The MODS section: MOD1 and MOD3 side by side, then MOD2 and MOD4, each
/// with its type, DEST and parameters, at the columns of the fields above.
fn append_mods(
    song: &Song,
    instrument: &Instrument,
    params: &m8fmt::SynthParams,
    lines: &mut Vec<Line>,
) {
    lines.resize_with(MODS_LINE - 1, Line::default);
    let mut header = Line::default();
    header.put(1, "MODS", CellRole::Label);
    lines.push(header);
    let dests = mod_destinations(instrument);
    let mut block = vec![Line::default(); 2 * MODS_PAIR_GAP - 1];
    for (slot, m) in params.mods.iter().enumerate() {
        let (kind, bytes) = mod_bytes(m);
        let top = (slot % 2) * MODS_PAIR_GAP;
        let (label_col, value_col) = if slot < 2 {
            (1, 9)
        } else {
            (19, MODS_RIGHT_VALUE)
        };
        let kind_text = MOD_TYPE_NAMES
            .get(usize::from(kind))
            .copied()
            .unwrap_or("???");
        block[top].put(label_col, &format!("MOD{}", slot + 1), CellRole::Label);
        block[top].put(
            value_col,
            &format!("{kind:02X}{kind_text}"),
            CellRole::Value,
        );
        let dest = bytes[0] & 0x0F;
        let dest_name = dests.get(usize::from(dest)).copied().unwrap_or("???");
        block[top + 1].put(label_col, "DEST", CellRole::Label);
        block[top + 1].put(
            value_col,
            &format!("{dest:02X}{dest_name}"),
            CellRole::Value,
        );
        let labels = MOD_PARAM_LABELS
            .get(usize::from(kind))
            .copied()
            .unwrap_or_default();
        for (k, label) in labels.iter().enumerate().filter(|(_, l)| !l.is_empty()) {
            block[top + 2 + k].put(label_col, label, CellRole::Label);
            let text = mod_param_text(song, kind, k, bytes[1 + k]);
            block[top + 2 + k].put(value_col, &text, CellRole::Value);
        }
    }
    lines.extend(block);
}

fn hex(value: u8) -> String {
    format!("{value:02X}")
}

/// Seven rows: `left` at column 1, and AMP, LIM, PAN, DRY, MFX, DEL, REV at
/// column 19, as on the FMSYNTH and MACROSYNTH screens.
fn with_mixer_rows(
    mut lines: Vec<Line>,
    left: [(&str, String); 7],
    p: &m8fmt::SynthParams,
) -> Vec<Line> {
    let right = [
        ("AMP", hex(p.amp)),
        ("LIM", limit_label(p.limit)),
        ("PAN", hex(p.mixer_pan)),
        ("DRY", hex(p.mixer_dry)),
        ("MFX", hex(p.mixer_mfx)),
        ("DEL", hex(p.mixer_delay)),
        ("REV", hex(p.mixer_reverb)),
    ];
    for ((label, value), (right_label, right_value)) in left.into_iter().zip(right) {
        let mut line = Line::default();
        line.put(1, label, CellRole::Label);
        line.put(9, &value, CellRole::Value);
        line.put(19, right_label, CellRole::Label);
        line.put(23, &right_value, CellRole::Value);
        lines.push(line);
    }
    lines
}

/// MIDI OUT and EXTERNAL: the common settings only. Their own fields need
/// MIDI out and audio in, which ZeroTracker does not have yet, and their
/// byte layout is not checked against the M8.
fn unplayed_view(index: u8, kind: &str, name: &str, (transpose, table_tick): (bool, u8)) -> Frame {
    let mut lines = instrument_intro(index, kind, name, Some((transpose, table_tick, 0xFF)));
    let mut line = Line::default();
    line.put(1, "NOT PLAYED BY ZEROTRACKER YET", CellRole::Label);
    lines.push(line);
    lines.resize_with(14, Line::default);
    Frame { lines }
}

fn none_view(index: u8) -> Frame {
    let mut lines = instrument_intro(index, "NONE", "", None);
    lines.resize_with(14, Line::default);
    Frame { lines }
}

fn table_view(song: &Song, index: u8) -> Frame {
    let table = &song.tables[usize::from(index)];
    let mut lines = vec![title("TABLE", Some(index))];
    let mut header = Line::default();
    header.push("   N  V  FX1   FX2   FX3", CellRole::Label);
    lines.push(header);
    let pack = song
        .instruments
        .get(usize::from(index))
        .unwrap_or(&Instrument::None)
        .instr_command_text(song.version);
    for (row, step) in table.steps.iter().enumerate() {
        let mut line = Line::default();
        line.push(&format!(" {row:X} "), CellRole::Label);
        value_or_empty(&mut line, step.transpose);
        line.push(" ", CellRole::Label);
        value_or_empty(&mut line, step.velocity);
        for fx in [step.fx1, step.fx2, step.fx3] {
            line.push(" ", CellRole::Label);
            line.push(&effect(song, fx, pack), CellRole::Effect);
        }
        lines.push(line);
    }
    Frame { lines }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Labels as the target device shows them (read on 15/10/2026).
    #[test]
    fn slice_labels_match_the_m8() {
        assert_eq!(slice_label(0x00), "00OFF");
        assert_eq!(slice_label(0x01), "01FILE");
        assert_eq!(slice_label(0x02), "02(002)");
        assert_eq!(slice_label(0x14), "14(020)");
    }

    // Bytes read back from a song saved by the target device: 05 1>RAT,
    // 09 1>PIT, 0D 1>FBK, 10 4>FBK.
    #[test]
    fn fm_mod_labels_cover_all_four_destinations() {
        assert_eq!(fm_mod_label(0x00), "-----");
        assert_eq!(fm_mod_label(0x02), "2>LEV");
        assert_eq!(fm_mod_label(0x05), "1>RAT");
        assert_eq!(fm_mod_label(0x09), "1>PIT");
        assert_eq!(fm_mod_label(0x0D), "1>FBK");
        assert_eq!(fm_mod_label(0x10), "4>FBK");
    }

    #[test]
    fn filter_and_limit_carry_their_names() {
        let types = ["OFF", "LOWPASS"];
        assert_eq!(filter_label(&types, 1), "01LOWPASS");
        assert_eq!(filter_label(&types, 9), "09???");
        assert_eq!(limit_label(m8fmt::LimitType(0)), "00CLIP");
        assert_eq!(limit_label(m8fmt::LimitType(0x20)), "20???");
    }
}
