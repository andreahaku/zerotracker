#!/usr/bin/env bash
# Builds content/starter (not in git) from a content tree laid out as
# ~/ZeroTracker: what the .deb carries and the app copies on its first start.
# usage: make-starter.sh CONTENT_DIR
set -euo pipefail
SRC=${1:?usage: make-starter.sh CONTENT_DIR}
REPO=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
OUT=$REPO/content/starter
rm -rf "$OUT"
mkdir -p "$OUT/Songs/Workshop" "$OUT/Samples/Starter"

cp -a "$SRC/Songs/Workshop/mikey303 - SLSKERS DEMO" "$OUT/Songs/Workshop/"
# Only presets whose licence is documented; the others wait for their
# authors' permission.
mkdir -p "$OUT/Instruments"
cp -a "$SRC/Instruments/mikey303 Ravepack" "$OUT/Instruments/"

# A small kit of samples with a clear licence: FLOOR BABA's own synth
# recordings (CC0) and two sliced kits of the Jam 41 Space pack (CC0, freesound).
FB=$SRC/Samples/FLOOR_BABA-BABA_LAB_INGREDIENTS
J41=$SRC/Samples/M8CJ_Jam_41_Space-Sample_Pack
KIT=$OUT/Samples/Starter
mkdir -p "$KIT/Drums" "$KIT/Bass" "$KIT/Pads"
cp -a "$FB/DRUMS/WEST PEST" "$KIT/Drums/WEST PEST"
cp -a "$FB/DRUMS/MICROFREAK" "$KIT/Drums/MICROFREAK"
cp -a "$J41/kits/adhdreaming-kit__p35078+33@.wav" "$J41/kits/tamb__tairblenn__545513+9@.wav" "$KIT/Drums/"
cp -a "$FB/BASS/MICROFREAK/." "$KIT/Bass/"
for f in "UNDERWATER Cmin9.wav" "UNDERWATER Cmaj7.wav"; do
    cp -a "$FB/PADS/FLOOR BABA d05 UNDERWATER PADS/$f" "$KIT/Pads/"
done
cp -a "$FB/readme.txt" "$KIT/FLOOR BABA readme.txt"
cp -a "$J41/README.md" "$KIT/Jam 41 Space README.md"
cp -a "$REPO/tools/packs/CONTENT-starter.md" "$OUT/CONTENT.md"
du -sh "$OUT"
find "$OUT" -type f | sed 's/.*\.//' | sort | uniq -c
