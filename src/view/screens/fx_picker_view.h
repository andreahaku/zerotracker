/*
 * The FX command picker as graphics (out of the
 * text-only look): the commands as a grid of chips, three letters each, the
 * chosen one on the cursor colour with its number in the title. The arrows
 * move on the grid (the viewer's picker_input); EDIT or ENTER chooses, OPTION
 * or ESC cancels. One LVGL object, one draw callback.
 */
#pragma once

#include "lvgl.h"
#include "zt_viewer.h"

#include <cstdint>

namespace screen {

class FxPickerView {
public:
    struct Colors {
        uint32_t bg, panel, fg, dim, hi, accent, cursor, on_cursor;
    };
    struct Fonts {
        const lv_font_t* label;  // chips and title
        const lv_font_t* small;
    };

    void create(lv_obj_t* parent, int x, int y, int w, int h, const Fonts& fonts, const Colors& colors);
    void set_visible(bool visible);
    // `title` in the APP language.
    void set(const ZtFxPicker& picker, const char* title);

private:
    static void draw_cb(lv_event_t* e);
    void draw(lv_layer_t* layer, const lv_area_t& box) const;

    lv_obj_t* obj_{nullptr};
    Fonts f_{};
    Colors c_{};
    ZtFxPicker p_{};
    const char* title_{""};
    int top_row_{0};
    int h_{0};
};

} // namespace screen
