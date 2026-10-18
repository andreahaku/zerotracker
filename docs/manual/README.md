# ZeroTracker: getting started

ZeroTracker is a music tracker for the M5Stack CardputerZero. It reads and writes M8 song
(`.m8s`) and instrument (`.m8i`) files and follows most of the M8 workflow: a song plays chains,
a chain plays phrases, a phrase plays notes on instruments. This guide covers how ZeroTracker
is organised. To learn the tracker workflow itself, start with the resources at the end.

ZeroTracker is an independent project, not affiliated with Dirtywave.

## Install and first start

1. Install ZeroTracker from the CardputerZero app store, or copy the `.deb` from the releases
   page and run `sudo apt install ./ZeroTracker_*.deb`.
2. Open it from the launcher.
3. At the first start ZeroTracker creates `~/ZeroTracker` and copies the starter content into it:
   a song, 503 instruments and a small CC0 sample kit. Files already there stay as they are.

The folder holds everything:

| Folder | What |
|---|---|
| `Songs/` | songs (`.m8s`); a song may sit in its own folder with its samples |
| `Instruments/` | instruments (`.m8i`) |
| `Samples/` | samples (`.wav`); `Samples/REC/` gets your recordings |
| `Renders/` | WAV files from RENDER |
| `CONTENT.md` | authors and licences of the content that came with the app |

## Keys

| Key | Action |
|---|---|
| F X Z C (or the arrow keys) | move; in edit mode, change the value |
| ENTER | the field's action: open, type a name, choose, edit |
| ESC | cancel, or go up one level; held 3 s, quit |
| 4 | next page; held, the page before |
| 5 6 7 8 | the four actions named in the bottom bar |
| L (or 5) held + arrows | change a value |
| DEL | clear the field |
| SPACE | play and stop |
| Ctrl + Z / Y | undo, redo |
| Ctrl + S | save |
| Ctrl + C / V | copy, paste |
| Fn + A / S / D | mute, volume down, volume up |
| SYM + 4..8 | what that key does on this page |

## Pages

Key 4 goes through the pages. The page name is on the tab at the bottom left.

- **HOME**: RESUME the last song, LOAD a song, NEW song, LIBRARY.
- **TIME**: the song as eight lanes. ENTER opens CHAIN and PHRASE side by side, then the
  INSTRUMENT, its EQ, its sample in the sample editor, and the TABLE.
- **MIX**: meters, track volumes, mute and solo, the master and its limiter. OSCIL shows the
  waves of the playing tracks.
- **INST**: a card per track with what it plays, then every instrument of the song.
- **SONG**: name, tempo, transpose, SAVE, NEW, LOAD, and RENDER to WAV (the mix, a stem per
  track, or both).
- **APP**: font size, colour theme, spectrum, language, MANUAL (this guide), and the full
  instructions.
- **LIB**: LIBRARY, packs of songs, instruments and samples to download.

## A first sound in five steps

1. HOME, key 8: NEW. A new empty song opens on TIME.
2. ENTER on the first cell of track 1: a new chain opens, ENTER again: a new phrase.
3. ENTER on the NOTE column writes a note; L held with the arrows changes it.
4. Move to the instrument column and press ENTER to open the instrument. Change its TYPE (FM,
   WAV, MACRO, HYPER, SAMPLER) and listen with SPACE.
5. Ctrl + S saves the song in `Songs/`.

## LIBRARY

LIB, after APP (or key 5 on HOME), lists packs you can download. Each row shows what the pack
holds, its licence, and whether it is installed (a green check) or not (a cloud and its size).

- 5 **GET** downloads and installs a pack. ESC stops it; GET again resumes where it stopped.
- 5 **REMOVE** on an installed pack deletes its files. Files you changed stay.
- 6 **INFO** shows the pack's description, authors and licence.
- 7 **RELOAD** fetches the list again.

Packs are downloaded from their original hosts where possible, such as the Internet Archive.
Nothing is ever overwritten.

## Differences from the M8

ZeroTracker is built by listening and measuring, not from the M8's code, so its sound is close
to the M8 but not identical. Levels can differ by 1-2 dB per track, filters at high resonance,
distortion and effects are our own designs, and WAVSYNTH wavetables play generic waveforms.
MIDI and external instruments do not play. The keys are the CardputerZero's, not the M8's
button combinations.

## Learn the M8 workflow

Material written for the M8 applies to ZeroTracker, with different keys.

- [M8 Operation Manual](https://dirtywave.com/pages/resources-downloads), Dirtywave. The
  reference for every screen and FX command.
- [Dirtywave M8 Tutorial: Learn the Basics & Best Workflow](https://www.youtube.com/watch?v=SJQzd0LtuXY),
  mylarmelodies.
- [Dirtywave M8 Tracker - Introduction](https://www.youtube.com/watch?v=jMxDGkXlb0k), Red Means
  Recording.
- [Dirtywave M8 Tutorial #1](https://www.youtube.com/watch?v=Xe0NnfjcJ_g), Stephen McLeod, the
  first of a beginner series.
- [Dirtywave M8 basic workflow](https://www.youtube.com/watch?v=jyvayCbfRq4), Avrilcadabra.
- [The M8 Companion](https://cs.uwaterloo.ca/~plragde/flane/TM8C/), a free book that teaches
  the M8 through complete songs.
- [Learn FM Synthesis with M8 Tracker](https://www.youtube.com/watch?v=r-RvH-jI9fQ), Impbox.

More videos, guides, presets, sample packs and communities: [resources.md](resources.md).
