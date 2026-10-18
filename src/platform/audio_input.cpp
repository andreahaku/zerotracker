/*
 * Audio input. See audio_input.h.
 */
#include "audio_input.h"

#include <algorithm>
#include <atomic>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <mutex>
#include <thread>

#if USE_DESKTOP
#include <SDL.h>
#else
#include <alsa/asoundlib.h>
#endif

namespace platform::audio_input {
namespace {

constexpr std::size_t kBlock = 512;                 // frames per waveform peak
constexpr std::size_t kMaxFrames = kRate * 60 * 5;  // five minutes at most

std::mutex g_mutex;
std::vector<int16_t> g_pcm;
std::vector<float> g_peaks;  // one per kBlock frames
float g_peak = 0.0f;
std::atomic<bool> g_recording{false};

// Appends captured frames (under the lock), with the peaks.
void keep(const int16_t* data, std::size_t n) {
    std::lock_guard<std::mutex> lock(g_mutex);
    const std::size_t room = kMaxFrames > g_pcm.size() ? kMaxFrames - g_pcm.size() : 0;
    n = std::min(n, room);
    for (std::size_t i = 0; i < n; ++i) {
        const float v = std::abs(data[i]) / 32768.0f;
        g_peak = std::max(g_peak, v);
        if (g_pcm.size() % kBlock == 0) g_peaks.push_back(0.0f);
        g_peaks.back() = std::max(g_peaks.back(), v);
        g_pcm.push_back(data[i]);
    }
}

#if USE_DESKTOP
SDL_AudioDeviceID g_device = 0;

void sdl_capture(void*, Uint8* stream, int length) {
    keep(reinterpret_cast<const int16_t*>(stream), static_cast<std::size_t>(length) / sizeof(int16_t));
}
#else
std::thread g_thread;
std::atomic<bool> g_stop{false};

void run_alsa(snd_pcm_t* pcm) {
    int16_t buf[256];
    while (!g_stop.load(std::memory_order_acquire)) {
        const snd_pcm_sframes_t n = snd_pcm_readi(pcm, buf, 256);
        if (n == -EPIPE) {
            snd_pcm_prepare(pcm);
            continue;
        }
        if (n < 0) {
            if (snd_pcm_recover(pcm, static_cast<int>(n), 1) < 0) break;
            continue;
        }
        keep(buf, static_cast<std::size_t>(n));
    }
    snd_pcm_close(pcm);
}
#endif

void put16(std::ofstream& out, uint16_t v) {
    const char b[2] = {static_cast<char>(v & 0xFF), static_cast<char>(v >> 8)};
    out.write(b, 2);
}

void put32(std::ofstream& out, uint32_t v) {
    put16(out, static_cast<uint16_t>(v & 0xFFFF));
    put16(out, static_cast<uint16_t>(v >> 16));
}

} // namespace

bool start(std::string& error) {
    if (g_recording) return true;
    {
        std::lock_guard<std::mutex> lock(g_mutex);
        g_pcm.clear();
        g_pcm.reserve(kRate * 10);
        g_peaks.clear();
        g_peak = 0.0f;
    }
#if USE_DESKTOP
    if (SDL_InitSubSystem(SDL_INIT_AUDIO) != 0) {
        error = SDL_GetError();
        return false;
    }
    SDL_AudioSpec wanted{};
    wanted.freq = kRate;
    wanted.format = AUDIO_S16SYS;
    wanted.channels = 1;
    wanted.samples = 512;
    wanted.callback = sdl_capture;
    SDL_AudioSpec got{};
    g_device = SDL_OpenAudioDevice(nullptr, 1, &wanted, &got, 0);
    if (g_device == 0) {
        error = SDL_GetError();
        return false;
    }
    SDL_PauseAudioDevice(g_device, 0);
#else
    const char* device = std::getenv("ZT_ALSA_CAPTURE");
    if (!device || !*device) device = "pipewire";
    snd_pcm_t* pcm = nullptr;
    int err = snd_pcm_open(&pcm, device, SND_PCM_STREAM_CAPTURE, 0);
    if (err < 0) {
        error = snd_strerror(err);
        return false;
    }
    err = snd_pcm_set_params(pcm, SND_PCM_FORMAT_S16_LE, SND_PCM_ACCESS_RW_INTERLEAVED, 1, kRate, 1, 50000);
    if (err < 0) {
        error = snd_strerror(err);
        snd_pcm_close(pcm);
        return false;
    }
    g_stop = false;
    g_thread = std::thread(run_alsa, pcm);
#endif
    g_recording = true;
    return true;
}

std::vector<int16_t> stop() {
    if (g_recording) {
#if USE_DESKTOP
        SDL_PauseAudioDevice(g_device, 1);
        SDL_CloseAudioDevice(g_device);
        g_device = 0;
#else
        g_stop = true;
        if (g_thread.joinable()) g_thread.join();
#endif
        g_recording = false;
    }
    std::lock_guard<std::mutex> lock(g_mutex);
    return std::move(g_pcm);
}

bool recording() {
    return g_recording;
}

std::size_t frames() {
    std::lock_guard<std::mutex> lock(g_mutex);
    return g_pcm.size();
}

float take_peak() {
    std::lock_guard<std::mutex> lock(g_mutex);
    const float p = g_peak;
    g_peak = 0.0f;
    return p;
}

std::vector<float> recent_peaks(std::size_t count) {
    std::lock_guard<std::mutex> lock(g_mutex);
    const std::size_t n = std::min(count, g_peaks.size());
    return {g_peaks.end() - static_cast<std::ptrdiff_t>(n), g_peaks.end()};
}

bool write_wav(const std::string& path, const std::vector<int16_t>& pcm) {
    std::ofstream out(path, std::ios::binary);
    if (!out) return false;
    const uint32_t bytes = static_cast<uint32_t>(pcm.size() * sizeof(int16_t));
    out.write("RIFF", 4);
    put32(out, 36 + bytes);
    out.write("WAVEfmt ", 8);
    put32(out, 16);
    put16(out, 1);  // PCM
    put16(out, 1);  // mono
    put32(out, kRate);
    put32(out, kRate * 2);
    put16(out, 2);
    put16(out, 16);
    out.write("data", 4);
    put32(out, bytes);
    for (int16_t v : pcm) put16(out, static_cast<uint16_t>(v));
    return static_cast<bool>(out);
}

} // namespace platform::audio_input
