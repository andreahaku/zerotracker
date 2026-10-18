/*
 * Graphic MIXER: nine strips, tracks 1-8 and
 * MIX. Each strip has its badge (red when selected), a stereo level meter
 * as a gradient from green through yellow and orange to red with a peak
 * mark, the volume and the M(ute)/S(olo) lights; MIX also shows the limiter
 * level as a line across its meter. One LVGL object, one draw callback.
 */
#pragma once

#include "lvgl.h"
#include "zt_viewer.h"

#include <array>
#include <cstdint>

namespace screen {

// Meter colour for a fraction 0..1 of full scale: green, yellow, orange, red;
// in a monochrome theme from `low` to `high` instead.
struct MeterScale {
    bool colorful;
    uint32_t low, high;
};
void set_meter_scale(const MeterScale& scale);
lv_color_t meter_color(float fraction);

class MixerView {
public:
    struct Colors {
        uint32_t badge, selected, text, dim, unlit, muted, solo, limiter;
        // Text on the badge, on the selected and mute fills, on the solo fill;
        // bright marks (the limiter line outside LIMIT mode).
        uint32_t on_badge, on_selected, on_solo, hi;
    };

    void create(lv_obj_t* parent, int x, int y, int w, int h, const lv_font_t* font,
                const lv_font_t* small_font, const Colors& colors);
    void set_visible(bool visible);
    // Levels: L and R per strip (tracks 0-7, master 8), 0..65535.
    void set(const ZtMixer& mixer, const std::array<uint16_t, 18>& levels);

private:
    static void draw_cb(lv_event_t* e);
    void draw(lv_layer_t* layer, const lv_area_t& box) const;
    void draw_meter(lv_layer_t* layer, int x, int y_top, int y_bottom, int lit, int peak,
                    bool dim) const;

    lv_obj_t* obj_{nullptr};
    const lv_font_t* font_{nullptr};
    const lv_font_t* small_{nullptr};
    Colors c_{};
    int w_{0}, h_{0};
    ZtMixer m_{};
    // Lit slices and peak-hold slice per meter (18 meters).
    std::array<int, 18> lit_{};
    std::array<int, 18> peak_{};
    std::array<uint32_t, 18> peak_at_{};
    int segments_{20};
    bool valid_{false};
};

} // namespace screen
