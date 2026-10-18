# Braids DSP subset

This directory contains the MIT-licensed oscillator and support sources used
by the MACROSYNTH adapter. The upstream source was taken from
`pichenettes/eurorack` at commit `f4cfa7697095a28d292bff758beb51deb49b5a86`.
Copyright and permission notices remain in each upstream source file; the
`stmlib/LICENSE` file is retained with its code.

The subset is limited to the files referenced by `build.rs`. It excludes the
AVR implementation, `third_party/STM`, `programming/serial`, device firmware,
and unrelated project assets. `zt_braids_wrapper.cc` is a ZeroTracker adapter
that exposes a fixed-storage C interface around the upstream oscillator.

The M8 mode-to-Braids mapping is an observed compatibility assumption, not a
claim about the M8 firmware implementation. The isolated oracle controls and
song comparisons document the behavior currently verified.
