/*
 * Audio input for REC. (the SAMPLER's record): 44.1 kHz, 16-bit mono from
 * the Cardputer's microphone. Device builds read ALSA through the PipeWire PCM
 * (or ZT_ALSA_CAPTURE); desktop builds use SDL2's capture device. One
 * recording at a time, on its own thread.
 */
#pragma once

#include <cstddef>
#include <cstdint>
#include <string>
#include <vector>

namespace platform::audio_input {

constexpr unsigned kRate = 44100;

// Opens the input and starts keeping what it hears. False (with `error`)
// when no input could be opened.
bool start(std::string& error);
// Stops and returns the recording (mono s16).
std::vector<int16_t> stop();
bool recording();
// Frames kept so far.
std::size_t frames();
// Peak 0..1 since the last call (for the level meter).
float take_peak();
// The latest `count` peaks of 512-frame blocks, oldest first (the live
// waveform).
std::vector<float> recent_peaks(std::size_t count);

// Writes `pcm` as a 16-bit mono WAV at kRate. False on a write error.
bool write_wav(const std::string& path, const std::vector<int16_t>& pcm);

} // namespace platform::audio_input
