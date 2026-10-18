/* C API for ZeroTracker's audio engine. Link libzerotracker_core.a. */
#ifndef ZT_ENGINE_H
#define ZT_ENGINE_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct ZtEngine ZtEngine;

/* Allocate once before starting audio. test_tone=1 emits a quiet 440 Hz sine;
 * test_tone=0 emits silence. Use sample_rate=44100 for the device backend.
 * Returns NULL for sample rates outside 8000..192000 Hz. */
ZtEngine *zt_engine_create(uint32_t sample_rate, int32_t test_tone);

/* Call only after the audio callback has stopped. NULL is accepted. */
void zt_engine_destroy(ZtEngine *ctx);

/* Fill frames of interleaved signed 16-bit PCM (S16_LE on the device).
 * channels must be 1 or 2. Output needs frames*channels writable samples.
 * Returns 1 on success, 0 for invalid arguments. A zero-frame call succeeds.
 * The same context must not be rendered/destroyed concurrently. This call
 * performs no allocation or locking. */
int32_t zt_engine_render(ZtEngine *ctx, int16_t *output, size_t frames,
                         uint32_t channels);

#define ZT_OUTPUT_WINDOW_FRAMES 2048
#define ZT_TRACK_OUTPUT_WINDOW_FRAMES 512

/* Copy the latest final master-output samples, oldest first, as normalized
 * mono floats. frames must be 1..ZT_OUTPUT_WINDOW_FRAMES; out needs frames
 * writable floats. Returns frames on success or 0 for invalid arguments,
 * insufficient history, or a concurrent overwrite. On failure, out is
 * unspecified. May run alongside rendering, but not engine destruction. */
size_t zt_engine_read_output(const ZtEngine *ctx, float *out, size_t frames);

/* Copy the latest post-track-fader mono waveform for track 0..7, oldest
 * first, as normalized floats. frames must be 1..ZT_TRACK_OUTPUT_WINDOW_FRAMES.
 * Returns frames or 0 for invalid arguments, insufficient history, or a
 * concurrent overwrite. May run alongside rendering, not destruction. */
size_t zt_engine_read_track_output(const ZtEngine *ctx, uint8_t track,
                                   float *out, size_t frames);

/* Read 18 linear peak values (0..65535), interleaved L/R for tracks 0..7,
 * then master L/R. Track peaks are post-track-fader; master peaks are
 * post-master-fader and post-effects. Returns 1 on success, 0 otherwise. */
int32_t zt_engine_track_peaks(const ZtEngine *ctx, uint16_t out[18]);

/* Read 16 linear peak values (0..65535), interleaved L/R for tracks 0..7.
 * Activity is measured after track volume but before MUTE/SOLO, so excluded
 * tracks remain visible. Returns 1 on success, 0 otherwise. */
int32_t zt_engine_track_activity(const ZtEngine *ctx, uint16_t out[16]);

/* The latest transport request wins. SONG starts all tracks at `row`;
 * CHAIN and PHRASE audition `index` on `track` from `row`. */
enum { ZT_STOP = 0, ZT_PLAY_SONG = 1, ZT_PLAY_CHAIN = 2,
       ZT_PLAY_PHRASE = 3 };
int32_t zt_engine_transport(ZtEngine *ctx, int32_t mode, uint8_t index,
                             uint8_t row, uint8_t track);

typedef struct ZtTrackStatus {
    uint8_t song_row;
    uint8_t chain;
    uint8_t chain_row;
    uint8_t phrase;
    uint8_t phrase_step;
    uint16_t peak;
    uint8_t active;
} ZtTrackStatus;

/* Read one track's playback position and peak level without locking.
 * track is 0..7. Returns 1 on success, 0 for invalid arguments. */
int32_t zt_engine_track_status(const ZtEngine *ctx, uint8_t track,
                               ZtTrackStatus *out);

/* Bit 0 is track 1; mute takes precedence over solo. The masks are live
 * state, not saved in the song. Both functions may run concurrently with
 * audio rendering, but not with destruction of the engine. */
int32_t zt_engine_set_track_flags(ZtEngine *ctx, uint8_t mute_mask,
                                   uint8_t solo_mask);
int32_t zt_engine_get_track_flags(const ZtEngine *ctx, uint8_t *mute_mask,
                                   uint8_t *solo_mask);

/* Bypass the master limiter for stem renders exported before the master bus. */
int32_t zt_engine_set_master_limiter_bypass(ZtEngine *ctx, int32_t bypass);

#ifdef __cplusplus
}
#endif

#endif
