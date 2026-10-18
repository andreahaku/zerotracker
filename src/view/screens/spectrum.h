/*
 * Spectrum of the master output for the top band: Hann window, radix-2 FFT,
 * log-spaced bands in dB. Runs on the UI thread from the engine's output tap.
 */
#pragma once

#include <complex>
#include <cstddef>
#include <vector>

namespace screen {

class Spectrum {
public:
    // `size` must be a power of two; `bands` log-spaced from lo_hz to hi_hz.
    Spectrum(size_t size, size_t bands, float rate, float lo_hz, float hi_hz);

    size_t size() const { return size_; }

    // Computes band levels 0..1 (-60..0 dBFS) from `size()` mono samples,
    // with a fall of `fall` per call so bars drop smoothly.
    const std::vector<float>& update(const float* samples, float fall);

    // Lets the bars fall when there is no new window.
    const std::vector<float>& decay(float fall);

    // Levels 0..1 (-80..-8 dBFS: one bin holds less than a band) at `count` log-spaced columns from lo_hz to
    // hi_hz, for a graph one column per pixel: the bins' peak where a column
    // spans several bins, interpolated in dB where it spans less than one.
    // Falls by `fall` per call; `samples` null lets the columns fall only.
    const std::vector<float>& columns(const float* samples, size_t count, float lo_hz, float hi_hz,
                                      float fall);

private:
    void transform(const float* samples);
    float bin_db(float bin) const;

    size_t size_;
    float rate_;
    std::vector<float> window_;
    std::vector<std::complex<float>> buf_;
    std::vector<size_t> edges_;  // FFT bin where each band starts (bands + 1)
    std::vector<float> levels_;
    std::vector<float> columns_;
};

} // namespace screen
