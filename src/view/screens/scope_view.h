/*
 * OSCILLOSCOPES: the latest samples of each of the eight tracks. Three
 * layouts, switched with VIEW: LANES, eight waves one
 * under the other across the full width (as on the M8); COLUMNS, eight
 * waves side by side running top to bottom; GRID, two rows of four. No
 * numbers or dividers, and silent tracks draw nothing: only the waves
 *. The
 * wave is normalised per track so quiet tracks still move; its colour is
 * the meter gradient (green, yellow, orange, red) of the track's real
 * level. Each frame starts at a rising zero crossing (a scope trigger), so
 * periodic waves stand still; lines are antialiased about 1.5 px wide over
 * the mean of the samples per pixel, with a short phosphor trail. Drawn into
 * a canvas pixel buffer and blitted once per frame: as
 * LVGL draw calls the dots were redone for every band of the partial
 * display buffer and the cells appeared one at a time on the device.
 */
#pragma once

#include "lvgl.h"

#include <array>
#include <cstdint>
#include <vector>

namespace screen {

class ScopeView {
public:
    static constexpr int kSamples = 512;
    enum class Layout { Lanes, Columns, Grid };

    void create(lv_obj_t* parent, int x, int y, int w, int h, uint32_t bg_color);
    void set_visible(bool visible);
    bool visible() const;
    // Samples of one track, -1..1, up to kSamples used.
    void set_track(int track, const float* samples, size_t count);
    // Redraws with the latest samples.
    void tick();
    void next_layout();
    Layout layout() const { return layout_; }

private:
    void render();
    void blend(int x, int y, uint32_t color, float coverage);
    void vline(int x, int y0, int y1, uint32_t color);
    void hline(int x0, int x1, int y, uint32_t color);
    // One wave in a box: along x (horizontal) or along y (vertical).
    void wave(int track, int x, int y, int w, int h, bool vertical);

    lv_obj_t* obj_{nullptr};
    std::vector<uint32_t> pixels_;
    uint32_t bg_{0};
    int w_{0}, h_{0};
    Layout layout_{Layout::Lanes};
    std::array<std::array<float, kSamples>, 8> samples_{};
    std::array<int, 8> count_{};
    // Meter fraction 0..1 of each track's peak, for the colour.
    std::array<float, 8> level_{};
};

} // namespace screen
