# Third-party code

- **CardputerZero Template** (https://github.com/CardputerZero/Template, commit 477fa6d), MIT.
  The C++ app shell (`src/`, `cmake/`, `config/`, `CMakeLists.txt`, `CMakePresets.json`) starts from it.
  License: `third_party/LICENSE.cardputerzero-template`. Fonts in `assets/fonts/` keep their own
  license file, `assets/fonts/LICENSE.txt`.
- **ZeroRadio** build container and script (`docker/cp0-build/`, `scripts/cp0-docker-build.sh`), MIT.
- **m8-file-parser** (https://github.com/Twinside/m8-file-parser), MIT: the base of `m8fmt`. NEW songs are generated from the format model at runtime; no song file is built into the app.
- **Cascadia Mono** (https://github.com/microsoft/cascadia-code, release 2407.24), SIL Open Font
  License 1.1: `assets/fonts/CascadiaMono-Regular.ttf` and `assets/fonts/CascadiaMono-Bold.ttf`, unmodified; license in
  `assets/fonts/LICENSE-CascadiaMono.txt`. The tracker grid font (Bold: playing chains, page tabs).
- **IBM Plex Mono** (https://github.com/IBM/plex, via Google Fonts), SIL Open Font License 1.1, Copyright 2017
  IBM Corp.: `assets/fonts/IBMPlexMono-Regular.ttf` and `assets/fonts/IBMPlexMono-Bold.ttf`, unmodified; license in
  `assets/fonts/LICENSE-IBMPlexMono.txt`. The app font.
- **Braids** oscillator DSP from Mutable Instruments' eurorack repository
  (https://github.com/pichenettes/eurorack, commit f4cfa76), MIT, Copyright Emilie Gillet: the
  MACROSYNTH shapes in `engine/vendor/braids/`. Each source keeps its notice; `stmlib` keeps its
  `LICENSE`. Only the files listed in `engine/build.rs` are included. ZeroTracker is not a Mutable
  Instruments product.
- **Icons**: Phosphor Icons (MIT) and Kenney Input Prompts (CC0 1.0), in `assets/fonts/`, with
  their terms in `assets/fonts/LICENSE.txt`.
- **LVGL** (https://github.com/lvgl/lvgl, v9.5.0), MIT, fetched at build time; its QR code
  widget includes Nayuki's QR Code generator (MIT).
- **Rust crates** from crates.io, each under its own licence (MIT, Apache-2.0, ISC, BSD-style,
  Unicode-3.0, and CDLA-Permissive-2.0 for the webpki-roots certificate list):
  `cargo metadata` lists them. The main ones are hound (WAV), ureq and rustls (HTTPS), zip,
  sha2, serde and serde_json.

# Content

The songs, instruments and samples the package copies into `~/ZeroTracker` on the first start
are not part of this repository. They belong to their authors and keep their own licences,
listed in `CONTENT.md` next to them: CC0 1.0 for the samples, CC BY-NC-SA 4.0 or the author's
own terms for the songs and instruments. LIBRARY packs carry the same information in their
catalog entry.
