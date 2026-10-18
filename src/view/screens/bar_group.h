/*
 * A group of vertical, bottom-aligned bars drawn by one LVGL object. Each
 * update invalidates one area: LVGL redraws the whole screen once more than
 * 32 areas are invalid in a frame (LV_INV_BUF_SIZE), which 40 separate bar
 * objects did at 20 Hz (35% of a core on the device).
 */
#pragma once

#include "lvgl.h"

#include <cstdint>
#include <vector>

namespace screen {

class BarGroup {
public:
    // A group over the area (x, y, w, h) of `parent`; bars are given in
    // columns relative to that area.
    void create(lv_obj_t* parent, int x, int y, int w, int h, uint32_t color);

    // Moves and resizes the area.
    void set_area(int x, int y, int w, int h);

    // Bar n spans [x, x + width) and grows up from the bottom of the area.
    void set_layout(std::vector<int> x, std::vector<int> width);

    // Heights in px, clamped to the area; redraws only when one changed.
    void set_heights(const std::vector<int>& heights);

    void set_visible(bool visible);
    int height() const { return h_; }
    lv_obj_t* obj() const { return obj_; }

private:
    static void draw_cb(lv_event_t* e);

    lv_obj_t* obj_{nullptr};
    int h_{0};
    uint32_t color_{0};
    std::vector<int> x_, width_, heights_;
};

} // namespace screen
