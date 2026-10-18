/*
 * INSTRUMENT as graphics: the M8 text layout, with the
 * labels in bold white, the values in colour and a bar beside each value
 * 00-FF (from the middle for the ones centred on 80, like DETUNE and PAN).
 * Lines alternate black and grey, as on CHAIN | PHRASE. One LVGL object,
 * one draw callback; the cell grid is the tracker's, so it follows the APP
 * font size.
 */
#pragma once

#include "lvgl.h"
#include "zt_viewer.h"

#include <array>
#include <cstdint>

namespace screen {

class InstrumentView {
public:
    struct Colors {
        uint32_t bg, panel, fg, dim, hi, accent, cursor, on_cursor, bar_track;
        std::array<uint32_t, 5> chain;  // the bars' colour, low to high value
    };

    void create(lv_obj_t* parent, int x, int y, int w, int h, const lv_font_t* font,
                const lv_font_t* bold_font, int cell_w, int cell_h, const Colors& colors);
    void set_visible(bool visible);
    // Edit mode: the cursor takes `color` (the theme's alert colour).
    void set_editing(bool editing, uint32_t color);
    // Redraws only when the page changed.
    void set(const ZtInstrumentPage& page);

private:
    static void draw_cb(lv_event_t* e);
    void draw(lv_layer_t* layer, const lv_area_t& box) const;

    lv_obj_t* obj_{nullptr};
    const lv_font_t* font_{nullptr};
    const lv_font_t* bold_{nullptr};
    Colors c_{};
    int w_{0}, h_{0};
    int cell_w_{8}, cell_h_{14};
    ZtInstrumentPage page_{};
    bool valid_{false};
    bool editing_{false};
    uint32_t edit_color_{0};
    // Column of the right-hand labels on this page, or a huge value.
    int right_col_{1 << 20};
};

} // namespace screen
