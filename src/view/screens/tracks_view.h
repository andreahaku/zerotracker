/*
 * TRK as graphics. With the cursor on the tracks: eight
 * cards, four by two, each with its number on a badge (red muted, the solo
 * colour soloed), the instrument's kind as an icon (the browser's waveform
 * for SAMPLER, its piano keys for the synths), slot and name, the detail
 * (sample or shape) and a live level bar. With the cursor in the POOL: the
 * song's instrument slots as rows, kind icon, slot, name and a mini bar each
 * for DRY, MFX, DEL and REV, then the EQ. One LVGL object, one draw callback.
 */
#pragma once

#include "lvgl.h"
#include "zt_viewer.h"

#include <array>
#include <cstdint>
#include <string>

namespace screen {

class TracksView {
public:
    struct Colors {
        uint32_t bg, panel, fg, dim, hi, accent, cursor, on_cursor, alert, badge, on_badge, muted, solo,
            on_solo, playing;
        std::array<uint32_t, 5> chain;
    };
    struct Fonts {
        const lv_font_t* label;
        const lv_font_t* small;
        const lv_font_t* icon;
    };

    void create(lv_obj_t* parent, int x, int y, int w, int h, const Fonts& fonts, const Colors& colors);
    void set_visible(bool visible);
    // Levels: L and R per track 0-7 (the mixer's), 0..65535. `pool_title`
    // and `empty` in the APP language.
    void set(const ZtTracksPage& page, const std::array<uint16_t, 18>& levels, const std::string& pool_title,
             const std::string& empty);

private:
    static void draw_cb(lv_event_t* e);
    void draw(lv_layer_t* layer, const lv_area_t& box) const;
    void draw_tracks(lv_layer_t* layer, const lv_area_t& box) const;
    void draw_pool(lv_layer_t* layer, const lv_area_t& box) const;

    lv_obj_t* obj_{nullptr};
    Fonts f_{};
    Colors c_{};
    ZtTracksPage page_{};
    std::array<uint8_t, 8> level_px_{};
    std::string pool_title_, empty_;
};

} // namespace screen
