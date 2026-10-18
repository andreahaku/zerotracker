/*
 * Audio output backend. See audio_output.h.
 */
#include "audio_output.h"

#include "zt_engine.h"

#include <atomic>
#include <algorithm>
#include <cmath>
#include <chrono>
#include <cstddef>
#include <cstdint>
#include <cstdlib>
#include <cstdio>
#include <limits>
#include <system_error>
#include <thread>

#if USE_DESKTOP
#include <SDL.h>
#else
#include <alsa/asoundlib.h>
#endif

namespace platform::audio {
namespace {

constexpr unsigned kSampleRate = 44100;
constexpr unsigned kChannels = 2;
constexpr std::size_t kPeriodFrames = 256;

std::thread g_thread;
std::atomic<bool> g_stop{false};

#if USE_DESKTOP
struct SdlAudioState {
    ZtEngine* engine;
    std::uint64_t remaining_frames;
    std::uint64_t rendered_frames = 0;
    std::atomic<bool> done{false};
    unsigned render_errors = 0;
};

void sdl_audio_callback(void* userdata, Uint8* stream, int length) {
    auto* state = static_cast<SdlAudioState*>(userdata);
    SDL_memset(stream, 0, static_cast<std::size_t>(length));
    const auto frames = static_cast<std::size_t>(length) / (sizeof(int16_t) * kChannels);
    const auto render_frames = std::min<std::uint64_t>(frames, state->remaining_frames);
    if (render_frames > 0 &&
        zt_engine_render(state->engine, reinterpret_cast<int16_t*>(stream),
                         static_cast<std::size_t>(render_frames), kChannels) != 1) {
        ++state->render_errors;
        state->done.store(true, std::memory_order_release);
        return;
    }
    state->remaining_frames -= render_frames;
    state->rendered_frames += render_frames;
    if (state->remaining_frames == 0) {
        state->done.store(true, std::memory_order_release);
    }
}

void run_sdl(double seconds, ZtEngine* external) {
    if (SDL_InitSubSystem(SDL_INIT_AUDIO) != 0) {
        std::fprintf(stderr, "[audio] SDL_InitSubSystem: %s\n", SDL_GetError());
        return;
    }

    ZtEngine* engine = external ? external : zt_engine_create(kSampleRate, 1);
    if (!engine) {
        SDL_QuitSubSystem(SDL_INIT_AUDIO);
        return;
    }

    const auto frames = external ? std::numeric_limits<std::uint64_t>::max()
                                 : static_cast<std::uint64_t>(seconds * kSampleRate);
    SdlAudioState state{engine, frames};
    SDL_AudioSpec wanted{};
    wanted.freq = static_cast<int>(kSampleRate);
    wanted.format = AUDIO_S16LSB;
    wanted.channels = kChannels;
    wanted.samples = kPeriodFrames;
    wanted.callback = sdl_audio_callback;
    wanted.userdata = &state;

    SDL_AudioSpec obtained{};
    const SDL_AudioDeviceID device = SDL_OpenAudioDevice(nullptr, 0, &wanted, &obtained, 0);
    if (device == 0) {
        std::fprintf(stderr, "[audio] SDL_OpenAudioDevice: %s\n", SDL_GetError());
        if (!external) {
            zt_engine_destroy(engine);
        }
        SDL_QuitSubSystem(SDL_INIT_AUDIO);
        return;
    }

    SDL_PauseAudioDevice(device, 0);
    while (!state.done.load(std::memory_order_acquire) &&
           !g_stop.load(std::memory_order_relaxed)) {
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }
    SDL_PauseAudioDevice(device, 1);
    SDL_CloseAudioDevice(device);
    if (!external) {
        zt_engine_destroy(engine);
    }
    SDL_QuitSubSystem(SDL_INIT_AUDIO);
    std::fprintf(stderr, "[audio] SDL output done: %llu frames, %u render errors\n",
                 static_cast<unsigned long long>(state.rendered_frames), state.render_errors);
}
#else
void run_alsa(double seconds, ZtEngine* external) {
    snd_pcm_t* pcm = nullptr;
    const char* device = std::getenv("ZT_ALSA_DEV");
    if (!device || !*device) {
        device = "pipewire";
    }
    int err = snd_pcm_open(&pcm, device, SND_PCM_STREAM_PLAYBACK, 0);
    if (err < 0) {
        std::fprintf(stderr, "[audio] snd_pcm_open(%s): %s\n", device, snd_strerror(err));
        return;
    }
    // 4 periods of 256 frames: ~23 ms of buffer.
    err = snd_pcm_set_params(pcm, SND_PCM_FORMAT_S16_LE, SND_PCM_ACCESS_RW_INTERLEAVED,
                             kChannels, kSampleRate, 1, 23220);
    if (err < 0) {
        std::fprintf(stderr, "[audio] snd_pcm_set_params: %s\n", snd_strerror(err));
        snd_pcm_close(pcm);
        return;
    }

    snd_pcm_uframes_t buffer_frames = 0;
    snd_pcm_uframes_t period_frames = 0;
    err = snd_pcm_get_params(pcm, &buffer_frames, &period_frames);
    if (err < 0) {
        std::fprintf(stderr, "[audio] snd_pcm_get_params: %s\n", snd_strerror(err));
    }

    ZtEngine* engine = external ? external : zt_engine_create(kSampleRate, 1);
    if (!engine) {
        snd_pcm_close(pcm);
        return;
    }

    int16_t buffer[kPeriodFrames * kChannels];
    const auto total = external ? std::numeric_limits<unsigned long>::max()
                                : static_cast<unsigned long>(seconds * kSampleRate);
    unsigned long written = 0;
    unsigned underruns = 0;
    while (written < total && !g_stop.load(std::memory_order_relaxed)) {
        const auto block_frames = std::min<std::size_t>(kPeriodFrames, total - written);
        if (zt_engine_render(engine, buffer, block_frames, kChannels) != 1) {
            std::fprintf(stderr, "[audio] engine render failed\n");
            break;
        }
        std::size_t offset = 0;
        while (offset < block_frames && !g_stop.load(std::memory_order_relaxed)) {
            snd_pcm_sframes_t n = snd_pcm_writei(
                pcm, buffer + offset * kChannels, block_frames - offset);
            if (n == -EPIPE) {
                ++underruns;
            }
            if (n < 0) {
                n = snd_pcm_recover(pcm, static_cast<int>(n), 1);
                if (n < 0) {
                    std::fprintf(stderr, "[audio] write: %s\n", snd_strerror(static_cast<int>(n)));
                    g_stop.store(true, std::memory_order_relaxed);
                    break;
                }
                continue;
            }
            if (n == 0) {
                std::fprintf(stderr, "[audio] write made no progress\n");
                g_stop.store(true, std::memory_order_relaxed);
                break;
            }
            offset += static_cast<std::size_t>(n);
            written += static_cast<unsigned long>(n);
        }
    }
    snd_pcm_drain(pcm);
    snd_pcm_close(pcm);
    if (!external) {
        zt_engine_destroy(engine);
    }
    std::fprintf(stderr,
                 "[audio] output done: %lu frames, %u underruns, PCM %s, buffer %lu frames, period %lu frames\n",
                 written, underruns, device, static_cast<unsigned long>(buffer_frames),
                 static_cast<unsigned long>(period_frames));
}
#endif

} // namespace

bool start_test_tone(double seconds) {
    stop();
    if (!std::isfinite(seconds) || seconds <= 0.0 || seconds > 600.0) {
        return false;
    }
    g_stop.store(false, std::memory_order_relaxed);
    try {
#if USE_DESKTOP
        g_thread = std::thread(run_sdl, seconds, nullptr);
#else
        g_thread = std::thread(run_alsa, seconds, nullptr);
#endif
    } catch (const std::system_error& error) {
        std::fprintf(stderr, "[audio] start thread: %s\n", error.what());
        return false;
    }
    return true;
}

bool start_engine(ZtEngine* engine) {
    if (!engine) {
        return false;
    }
    stop();
    g_stop.store(false, std::memory_order_relaxed);
    try {
#if USE_DESKTOP
        g_thread = std::thread(run_sdl, 0.0, engine);
#else
        g_thread = std::thread(run_alsa, 0.0, engine);
#endif
    } catch (const std::system_error& error) {
        std::fprintf(stderr, "[audio] start thread: %s\n", error.what());
        return false;
    }
    return true;
}

void stop() {
    g_stop = true;
    if (g_thread.joinable()) {
        g_thread.join();
    }
}

} // namespace platform::audio
