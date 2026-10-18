// ZeroTracker adapter for the MIT-licensed Braids macro oscillator.
// Copyright 2012 Emilie Gillet.

#include <cstdint>
#include <cstring>
#include <new>

#include "braids/macro_oscillator.h"

namespace {

constexpr size_t kBlockSize = 24;

struct Voice {
  braids::MacroOscillator oscillator;
  int16_t block[kBlockSize];
  uint8_t block_position;
  int16_t base_pitch;
  int16_t transpose_pitch_units;
};

constexpr size_t kStorageSize = 17216;
static_assert(sizeof(Voice) <= kStorageSize, "Braids voice storage is too small");
static_assert(alignof(Voice) <= 8, "Braids voice storage alignment is too small");

}  // namespace

extern "C" {

size_t zt_braids_voice_size() { return sizeof(Voice); }

void zt_braids_voice_init(
    void* storage,
    uint8_t shape,
    uint8_t timbre,
    uint8_t color,
    int16_t pitch) {
  std::memset(storage, 0, sizeof(Voice));
  Voice* voice = new (storage) Voice();
  voice->oscillator.Init();
  voice->oscillator.set_shape(
      static_cast<braids::MacroOscillatorShape>(shape));
  voice->oscillator.set_pitch(pitch);
  voice->oscillator.set_parameters(
      static_cast<int16_t>(timbre << 7),
      static_cast<int16_t>(color << 7));
  voice->oscillator.Strike();
  voice->block_position = kBlockSize;
  voice->base_pitch = pitch;
  voice->transpose_pitch_units = 0;
}

void zt_braids_voice_transpose_pitch(void* storage, int16_t pitch_units) {
  Voice* voice = static_cast<Voice*>(storage);
  voice->transpose_pitch_units = pitch_units;
  int32_t pitch = voice->base_pitch + pitch_units;
  if (pitch < 0) pitch = 0;
  if (pitch > 140 * 128) pitch = 140 * 128;
  voice->oscillator.set_pitch(static_cast<int16_t>(pitch));
}

void zt_braids_voice_set_parameters(void* storage, uint8_t timbre, uint8_t color) {
  Voice* voice = static_cast<Voice*>(storage);
  voice->oscillator.set_parameters(
      static_cast<int16_t>(timbre << 7),
      static_cast<int16_t>(color << 7));
}

float zt_braids_voice_frame(void* storage) {
  Voice* voice = static_cast<Voice*>(storage);
  if (voice->block_position == kBlockSize) {
    uint8_t sync[kBlockSize] = {};
    voice->oscillator.Render(sync, voice->block, kBlockSize);
    voice->block_position = 0;
  }
  return static_cast<float>(voice->block[voice->block_position++]) / 32768.0f;
}

}  // extern "C"
