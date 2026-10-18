# Instrument coverage in 6.x songs

Analyzed on 2026-10-13 using the typed writer in `m8fmt` after commit
`8356d44`. The private backup was read in place. No song, instrument,
sample path, or filename is stored in this repository.

## Method

For each parseable 6.x `.m8s`, write its `Song` onto an all-`0x00` buffer and
an all-`0xFF` buffer. An instrument byte is **uncovered** if either result
differs from the source byte. This is the same rule as `corpus-test`'s
`M8RT_COVERAGE=1`. Group the offsets relative to each 215-byte slot at
`0x13A3E + 215 * slot`. Classify slots from the parsed `Instrument` variant.

The input was every `.m8s` under the private `Backup` directory, not only
`Backup/Songs`. The 69 parseable 6.x songs contain 8,832 instrument slots:
7,507 empty, 890 Sampler, 180 FMSynth, 145 MacroSynth, 77 WavSynth, and
33 HyperSynth. The private corpus contains no MIDIOut or ExternalInst; the
upstream `m8fmt/examples/songs` fixtures supplement those types.

| Type | Slots | Uncovered bytes | Average per slot |
| --- | ---: | ---: | ---: |
| Empty | 7,507 | 0 | 0.0 |
| Sampler | 890 | 64,306 | 72.3 |
| FMSynth | 180 | 23,826 | 132.4 |
| MacroSynth | 145 | 23,077 | 159.2 |
| WavSynth | 77 | 12,307 | 159.8 |
| HyperSynth | 33 | 1,329 | 40.3 |
| **Total** | **8,832** | **124,845** | **1,809.3 per song** |

## What the offsets contain

| Type | Relative offsets | Classification |
| --- | --- | --- |
| All active types | `+1..+12` | 12-byte name string. The text is modelled, but unchanged strings preserve their original bytes. The coverage test therefore marks terminator and tail bytes as uncovered; it does **not** imply the name is an unknown field. |
| Sampler | `+87..+214` | 128-byte sample-path string. Uncovered bytes are terminator and tail bytes, including stale printable data and `0xFF`. The decoded path is modelled. |
| WavSynth, MacroSynth | `+33..+61` | Gap between the common synth parameters and associated EQ/modulators. `SynthParams::from_reader3` skips three bytes, reads one shape byte, then seeks to EQ and modulators. `SynthParams::write` only emits shape for 6.6+; the other bytes are not modelled. |
| Sampler | `+34..+61` | Same common-parameter gap; it begins one byte later because Sampler has one more instrument parameter. |
| HyperSynth | `+40..+61` | Same common-parameter gap after its chord and oscillator parameters. |
| FMSynth | `+61` | One-byte gap before associated EQ and modulators. |
| MIDIOut | `+19..+21`, `+42..+62` | Bytes explicitly skipped by `MIDIOut::write` and `SynthParams::write_modes`; not modelled. Seen in upstream fixtures, absent from the private corpus. |
| ExternalInst | `+41..+61` | Common-parameter gap. Seen in upstream fixtures, absent from the private corpus. |
| All active types | Some of `+68`, `+74`, `+80`, `+86` | Last byte of a six-byte modulator record. `Mod::write` advances six bytes but four-parameter modulators write only five bytes including the type/destination byte. The skipped byte is not represented by the typed modulator. The exact offset depends on which modulator occupies the record. |
| WavSynth, MacroSynth, FMSynth, MIDIOut, ExternalInst | Usually `+87..+214` | 128-byte tail beyond these types' writer. It is not a string field for these types. Most bytes are zero; some source slots have nonzero residual bytes. The writer leaves the tail untouched. |
| HyperSynth | `+199..+214` | Sixteen bytes after the 16 typed chords, left untouched. |

Across the private corpus, **38,343 uncovered string bytes are at or after
the first `0x00` or `0xFF` terminator**. None is inside the decoded text.
This includes names and Sampler paths. It explains why preserving raw string
fields is needed for a byte-identical overlay round trip.

The largest non-string gaps are `+33..+61` (or the type-specific equivalent)
and the 128-byte tails of non-Sampler instruments. Their contents must stay
in the original buffer until their meaning is established; zero-valued bytes
alone are not evidence that a new file may omit them. The empty-slot block is
already deterministic and fully covered by the writer in these 6.x songs.

## Reproduction

`corpus-test` reports the same 69 read songs and 124,845 uncovered instrument
bytes when run with `M8RT_COVERAGE=1` on all private backup `.m8s` files. The
type-specific grouping was computed by reading each song with `m8fmt`, writing
onto both fill buffers, and counting uncovered bytes in each parsed slot.
The temporary analysis program and its raw output live under
`/tmp/zt-instrument-coverage/`; they are not part of the repository.
