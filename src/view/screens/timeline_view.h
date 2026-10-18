/*
 * Graphic TIMELINE: eight lanes of equal
 * height, alternating black and dark grey, a round numbered badge per track, the song rows left to right as
 * chain numbers on a smooth green-to-indigo scale from 00 to 8F,
 * empty rows blank, the playing chain of each track on a
 * filled red pill. One LVGL object drawn in a single callback, so a change
 * costs one invalidation.
 */
#pragma once

#include "lvgl.h"
#include "zt_viewer.h"

#include <array>
#include <cstdint>

namespace screen {

class TimelineView {
public:
    struct Colors {
        uint32_t bg, cursor, selection, lane, badge, badge_text, muted, solo, playing;
        uint32_t on_playing, on_cursor;  // chain text on the playing and cursor fills
        std::array<uint32_t, 5> chain;   // chain scale, 00 to 8F
    };

    void create(lv_obj_t* parent, int x, int y, int w, int h, const lv_font_t* font,
                const lv_font_t* badge_font, const Colors& colors);
    void set_visible(bool visible);
    // Song rows that fit across the lanes.
    int visible_rows() const;
    // Redraws only when the snapshot changed.
    void set(const ZtTimeline& t);
    int height() const { return h_; }

private:
    static void draw_cb(lv_event_t* e);
    void draw(lv_layer_t* layer, const lv_area_t& box) const;

    lv_obj_t* obj_{nullptr};
    const lv_font_t* font_{nullptr};
    const lv_font_t* badge_font_{nullptr};
    Colors c_{};
    int w_{0}, h_{0};
    int lane_h_{16};
    int glyph_w_{8};
    int cell_w_{24};
    int cells_x_{20};
    ZtTimeline t_{};
    bool valid_{false};
};

} // namespace screen
