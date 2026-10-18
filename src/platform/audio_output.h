/*
 * Audio output backend. The backend owns the sound device and pulls PCM from
 * the Rust engine (zt_engine_render) on its own thread.
 */
#pragma once

#include "zt_engine.h"
#include <cstdint>

namespace platform::audio {

// Plays the engine test tone for `seconds` on a background thread.
// Device builds use ALSA through the PipeWire PCM (or ZT_ALSA_DEV); desktop
// builds use SDL2 audio.
// Returns false for an invalid duration or when the thread could not start.
bool start_test_tone(double seconds);

// Starts continuous playback from an engine owned by the caller. The engine
// must remain alive until stop() has joined the audio thread. One thread owns
// zt_engine_render; the UI may concurrently publish snapshots and transport.
bool start_engine(ZtEngine* engine);

// Joins the background thread, if any. Safe to call more than once.
void stop();

} // namespace platform::audio
