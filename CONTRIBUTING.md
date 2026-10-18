# Contributing to ZeroTracker

Thank you for your interest. Bug reports, songs that sound wrong, translations, documentation and
code are all welcome.

## Before you start

- **Bugs and sound differences**: open an issue. Say what you did, what you expected and what
  happened. For sound, attach the song (`.m8s`) and, if you can, a render from an M8 of the same
  song, and say which track sounds different.
- **New features**: open an issue first, so we can agree on the design before you write code.
  ZeroTracker follows the M8 workflow where it can and the CardputerZero keyboard where it must.
- **Translations**: the UI texts live next to the code in seven languages (English, Italian,
  French, German, Spanish, Japanese, Chinese). Bottom-bar labels are at most six characters.

## Clean-room rules

ZeroTracker must stay free of anyone else's protected material. These rules are not optional.

- Never use disassembled, decompiled or leaked M8 firmware, or knowledge taken from it.
- Do not add Dirtywave material: code, data, wavetables, samples, fonts, graphics or text from
  the manual beyond short references.
- Work from the file formats (m8-file-parser and saved files), from what you can hear and
  measure, and from public documentation.
- Do not commit songs, instruments or samples. Content for LIBRARY goes through a pack, with
  its author, source and licence stated, and only with a licence that allows redistribution.

By sending a contribution you confirm that you wrote it, or that you have the right to submit it
under the project's licence.

## Code

- Rust for the format, the engine and the core library (`m8fmt`, `m8view`, `m8edit`, `engine`,
  `ffi`); C++ with LVGL for the app (`src/`).
- Run `scripts/fetch-fixtures.sh` once, then `scripts/ci.sh` before you open a pull request: it
  runs `cargo fmt`, `clippy` with warnings as errors, the tests, and the desktop build.
- Add a test for each fix and each new behaviour. Engine changes that alter the sound need a
  before/after measurement in the pull request.
- Write code, comments and docs in English. Match the style of the code around your change.
- Keep a pull request to one topic. Describe what changed and how you checked it.

## Licence

ZeroTracker is licensed under the GPL-3.0-or-later; `m8fmt` under MIT. Your contributions are
licensed the same way as the files you change.
