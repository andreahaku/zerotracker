/*
 * Spectrum. See spectrum.h.
 */
#include "spectrum.h"

#include <algorithm>
#include <cmath>

namespace screen {

Spectrum::Spectrum(size_t size, size_t bands, float rate, float lo_hz, float hi_hz)
    : size_(size), rate_(rate), window_(size), buf_(size), edges_(bands + 1), levels_(bands, 0.0f) {
    constexpr float kPi = 3.14159265358979f;
    for (size_t i = 0; i < size; ++i) {
        window_[i] = 0.5f - 0.5f * std::cos(2.0f * kPi * i / (size - 1));
    }
    const float bin_hz = rate / size;
    for (size_t b = 0; b <= bands; ++b) {
        const float hz = lo_hz * std::pow(hi_hz / lo_hz, static_cast<float>(b) / bands);
        edges_[b] = std::clamp<size_t>(static_cast<size_t>(hz / bin_hz), 1, size / 2);
    }
    // At least one bin per band.
    for (size_t b = 1; b <= bands; ++b) {
        edges_[b] = std::max(edges_[b], std::min(edges_[b - 1] + 1, size / 2));
    }
}

void Spectrum::transform(const float* samples) {
    const size_t n = size_;
    for (size_t i = 0; i < n; ++i) {
        buf_[i] = {samples[i] * window_[i], 0.0f};
    }
    // Iterative radix-2 FFT: bit reversal, then butterflies.
    for (size_t i = 1, j = 0; i < n; ++i) {
        size_t bit = n >> 1;
        for (; j & bit; bit >>= 1) j ^= bit;
        j ^= bit;
        if (i < j) std::swap(buf_[i], buf_[j]);
    }
    for (size_t len = 2; len <= n; len <<= 1) {
        const float angle = -2.0f * 3.14159265358979f / len;
        const std::complex<float> step(std::cos(angle), std::sin(angle));
        for (size_t i = 0; i < n; i += len) {
            std::complex<float> w(1.0f, 0.0f);
            for (size_t k = 0; k < len / 2; ++k) {
                const auto u = buf_[i + k];
                const auto v = buf_[i + k + len / 2] * w;
                buf_[i + k] = u + v;
                buf_[i + k + len / 2] = u - v;
                w *= step;
            }
        }
    }
}

// dBFS of the transformed window at a fractional bin, interpolated in dB.
float Spectrum::bin_db(float bin) const {
    const float scale = 4.0f / size_;
    auto db = [&](size_t k) {
        const float m = std::abs(buf_[k]) * scale;
        return m > 0.0f ? 20.0f * std::log10(m) : -120.0f;
    };
    const float b = std::clamp(bin, 1.0f, static_cast<float>(size_ / 2 - 1));
    const size_t k = static_cast<size_t>(b);
    const float f = b - k;
    return db(k) * (1.0f - f) + db(k + 1) * f;
}

const std::vector<float>& Spectrum::update(const float* samples, float fall) {
    transform(samples);
    const size_t n = size_;
    // Peak magnitude per band, in dBFS (a full-scale sine with a Hann window
    // peaks at n / 4).
    const float scale = 4.0f / n;
    for (size_t b = 0; b + 1 < edges_.size(); ++b) {
        float peak = 0.0f;
        for (size_t k = edges_[b]; k < std::max(edges_[b + 1], edges_[b] + 1); ++k) {
            peak = std::max(peak, std::abs(buf_[k]) * scale);
        }
        const float db = peak > 0.0f ? 20.0f * std::log10(peak) : -120.0f;
        const float level = std::clamp((db + 60.0f) / 60.0f, 0.0f, 1.0f);
        levels_[b] = std::max(level, levels_[b] - fall);
    }
    return levels_;
}

const std::vector<float>& Spectrum::columns(const float* samples, size_t count, float lo_hz,
                                            float hi_hz, float fall) {
    columns_.resize(count, 0.0f);
    if (samples) transform(samples);
    const float bin_hz = rate_ / size_;
    for (size_t c = 0; c < count; ++c) {
        float level = 0.0f;
        if (samples) {
            const float t0 = static_cast<float>(c) / count, t1 = static_cast<float>(c + 1) / count;
            const float b0 = lo_hz * std::pow(hi_hz / lo_hz, t0) / bin_hz;
            const float b1 = lo_hz * std::pow(hi_hz / lo_hz, t1) / bin_hz;
            float db = bin_db(0.5f * (b0 + b1));
            for (size_t k = static_cast<size_t>(std::ceil(b0)); k < b1 && k < size_ / 2; ++k) {
                db = std::max(db, bin_db(static_cast<float>(k)));
            }
            level = std::clamp((db + 80.0f) / 72.0f, 0.0f, 1.0f);
        }
        columns_[c] = std::max(level, columns_[c] - fall);
    }
    return columns_;
}

const std::vector<float>& Spectrum::decay(float fall) {
    for (auto& l : levels_) l = std::max(0.0f, l - fall);
    return levels_;
}

} // namespace screen
