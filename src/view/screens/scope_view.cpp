/*
 * ScopeView. See scope_view.h.
 */
#include "scope_view.h"

#include "mixer_view.h"  // meter_color

#include <algorithm>
#include <cmath>

namespace screen {
namespace {

uint32_t argb(lv_color_t c) {
    return 0xFF000000u | (static_cast<uint32_t>(c.red) << 16) |
           (static_cast<uint32_t>(c.green) << 8) | c.blue;
}

} // namespace

void ScopeView::create(lv_obj_t* parent, int x, int y, int w, int h, uint32_t bg_color) {
    w_ = w;
    h_ = h;
    bg_ = 0xFF000000u | bg_color;
    pixels_.assign(static_cast<size_t>(w) * h, bg_);
    obj_ = lv_canvas_create(parent);
    lv_canvas_set_buffer(obj_, pixels_.data(), w, h, LV_COLOR_FORMAT_XRGB8888);
    lv_obj_set_pos(obj_, x, y);
    lv_obj_remove_flag(obj_, LV_OBJ_FLAG_CLICKABLE);
    for (auto& s : samples_) s.fill(0.0f);
    count_.fill(0);
    level_.fill(0.0f);
}

void ScopeView::next_layout() {
    layout_ = layout_ == Layout::Lanes ? Layout::Columns
            : layout_ == Layout::Columns ? Layout::Grid
                                         : Layout::Lanes;
    // No phosphor trail from the old layout.
    std::fill(pixels_.begin(), pixels_.end(), bg_);
}

void ScopeView::set_visible(bool visible) {
    if (!obj_) return;
    auto apply = [visible](lv_obj_t* o) {
        if (visible) lv_obj_remove_flag(o, LV_OBJ_FLAG_HIDDEN);
        else lv_obj_add_flag(o, LV_OBJ_FLAG_HIDDEN);
    };
    apply(obj_);
}

bool ScopeView::visible() const {
    return obj_ && !lv_obj_has_flag(obj_, LV_OBJ_FLAG_HIDDEN);
}

void ScopeView::set_track(int track, const float* samples, size_t count) {
    if (track < 0 || track >= 8) return;
    auto& out = samples_[track];
    if (!samples || count == 0) {
        count_[track] = 0;
        level_[track] = 0.0f;
        return;
    }
    count = std::min<size_t>(count, kSamples);
    float peak = 0.0f;
    for (size_t i = 0; i < count; ++i) peak = std::max(peak, std::fabs(samples[i]));
    // A silent track (below -60 dBFS) draws nothing, not even a flat line.
    if (peak < 0.001f) {
        count_[track] = 0;
        level_[track] = 0.0f;
        return;
    }
    // Shape normalised; colour from the real peak on the meter's -60..0 dB
    // scale.
    const float gain = 1.0f / std::max(peak, 0.01f);
    // Trigger, as on a hardware scope: start at the first rising zero
    // crossing in the first quarter, so a periodic wave stands still from
    // frame to frame; the window keeps three quarters of the samples.
    const size_t window = count * 3 / 4;
    size_t start = 0;
    for (size_t i = 1; i + window <= count && i < count / 4; ++i) {
        if (samples[i - 1] < 0.0f && samples[i] >= 0.0f) {
            start = i;
            break;
        }
    }
    for (size_t i = 0; i < window; ++i) out[i] = samples[start + i] * gain;
    count_[track] = static_cast<int>(window);
    level_[track] =
        peak > 0.0f ? std::clamp((20.0f * std::log10(peak) + 60.0f) / 60.0f, 0.0f, 1.0f) : 0.0f;
}

void ScopeView::tick() {
    if (!visible()) return;
    render();
    lv_obj_invalidate(obj_);
}

void ScopeView::vline(int x, int y0, int y1, uint32_t color) {
    if (x < 0 || x >= w_) return;
    if (y0 > y1) std::swap(y0, y1);
    for (int y = std::max(0, y0); y <= std::min(h_ - 1, y1); ++y) {
        pixels_[static_cast<size_t>(y) * w_ + x] = color;
    }
}

void ScopeView::hline(int x0, int x1, int y, uint32_t color) {
    if (y < 0 || y >= h_) return;
    if (x0 > x1) std::swap(x0, x1);
    for (int x = std::max(0, x0); x <= std::min(w_ - 1, x1); ++x) {
        pixels_[static_cast<size_t>(y) * w_ + x] = color;
    }
}

// Blends `color` over the pixel with coverage 0..1.
void ScopeView::blend(int x, int y, uint32_t color, float coverage) {
    if (x < 0 || x >= w_ || y < 0 || y >= h_ || coverage <= 0.0f) return;
    uint32_t& p = pixels_[static_cast<size_t>(y) * w_ + x];
    const float a = std::min(coverage, 1.0f);
    auto mix = [a](uint32_t dst, uint32_t src, int shift) {
        const float d = static_cast<float>((dst >> shift) & 0xFF);
        const float s = static_cast<float>((src >> shift) & 0xFF);
        return static_cast<uint32_t>(d + (s - d) * a + 0.5f) << shift;
    };
    p = 0xFF000000u | mix(p, color, 16) | mix(p, color, 8) | mix(p, color, 0);
}

// One wave in a box, antialiased: each step of the time axis covers the
// span between the previous and the current value, widened by the line's
// half thickness; every pixel gets the share of that span it covers. The
// value of a step is the mean of the samples falling in it (linear
// interpolation when a step holds less than one sample). `vertical` runs
// the time axis down instead of across.
void ScopeView::wave(int track, int x, int y, int w, int h, bool vertical) {
    constexpr float kHalfWidth = 0.75f;
    const int n = count_[track];
    if (n == 0) return;
    const auto& s = samples_[track];
    const int length = vertical ? h : w;
    const float amp = (vertical ? w : h) * 0.5f - 2.0f;
    const float mid = vertical ? x + w * 0.5f : y + h * 0.5f;
    const float per_step = static_cast<float>(n) / length;
    float prev = mid;
    for (int i = 0; i < length; ++i) {
        float v = 0.0f;
        if (per_step >= 1.0f) {
            const int b0 = static_cast<int>(i * per_step);
            const int b1 = std::max(b0 + 1, std::min(n, static_cast<int>((i + 1) * per_step)));
            for (int k = b0; k < b1; ++k) v += s[k];
            v /= static_cast<float>(b1 - b0);
        }
        else {
            const float at = i * per_step;
            const int i0 = static_cast<int>(at);
            const int i1 = std::min(i0 + 1, n - 1);
            v = s[i0] + (s[i1] - s[i0]) * (at - i0);
        }
        v = std::clamp(v, -1.0f, 1.0f);
        const float pos = mid + (vertical ? v : -v) * amp;
        const uint32_t color = argb(meter_color(level_[track] * (0.6f + 0.4f * std::fabs(v))));
        const float a = i == 0 ? pos : prev;
        const float lo = std::min(a, pos) - kHalfWidth, hi = std::max(a, pos) + kHalfWidth;
        const int t = (vertical ? y : x) + i;
        for (int p = static_cast<int>(std::floor(lo)); p <= static_cast<int>(std::floor(hi)); ++p) {
            const float coverage = std::min(hi, p + 1.0f) - std::max(lo, static_cast<float>(p));
            if (vertical) blend(p, t, color, coverage);
            else blend(t, p, color, coverage);
        }
        prev = pos;
    }
}

void ScopeView::render() {
    // Phosphor persistence: the last frame fades towards the background
    // instead of vanishing, so the motion looks continuous.
    constexpr uint32_t kKeep = 96;  // of 256: a short trail
    for (auto& p : pixels_) {
        auto fade = [](uint32_t px, uint32_t bg, int shift) {
            const int d = static_cast<int>((px >> shift) & 0xFF), b = static_cast<int>((bg >> shift) & 0xFF);
            return static_cast<uint32_t>(b + (d - b) * static_cast<int>(kKeep) / 256) << shift;
        };
        p = 0xFF000000u | fade(p, bg_, 16) | fade(p, bg_, 8) | fade(p, bg_, 0);
    }
    switch (layout_) {
    case Layout::Lanes: {
        const int lane_h = h_ / 8;
        for (int t = 0; t < 8; ++t) {
            wave(t, 0, t * lane_h + 1, w_, lane_h - 2, false);
        }
        break;
    }
    case Layout::Columns: {
        const int col_w = w_ / 8;
        for (int t = 0; t < 8; ++t) {
            wave(t, t * col_w + 1, 1, col_w - 2, h_ - 2, true);
        }
        break;
    }
    case Layout::Grid: {
        const int cell_w = w_ / 4, cell_h = h_ / 2;
        for (int t = 0; t < 8; ++t) {
            wave(t, (t % 4) * cell_w + 3, (t / 4) * cell_h + 4, cell_w - 6, cell_h - 8, false);
        }
        break;
    }
    }
}

} // namespace screen
