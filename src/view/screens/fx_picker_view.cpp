/*
 * FxPickerView. See fx_picker_view.h.
 */
#include "fx_picker_view.h"

#include <algorithm>
#include <cstdio>
#include <cstring>

namespace screen {
namespace {

constexpr int kTitleH = 20;
constexpr int kChipH = 21;
constexpr int kGap = 4;

void fill(lv_layer_t* layer, const lv_area_t& a, lv_color_t color, int radius = 0) {
    lv_draw_rect_dsc_t dsc;
    lv_draw_rect_dsc_init(&dsc);
    dsc.bg_color = color;
    dsc.bg_opa = LV_OPA_COVER;
    dsc.radius = radius;
    lv_draw_rect(layer, &dsc, &a);
}

void text(lv_layer_t* layer, const lv_area_t& a, const char* s, const lv_font_t* font,
          lv_color_t color, lv_text_align_t align = LV_TEXT_ALIGN_LEFT) {
    lv_draw_label_dsc_t dsc;
    lv_draw_label_dsc_init(&dsc);
    dsc.text = s;
    dsc.text_local = 1;  // drawing is deferred: LVGL keeps its own copy
    dsc.font = font;
    dsc.color = color;
    dsc.align = align;
    lv_draw_label(layer, &dsc, &a);
}

} // namespace

void FxPickerView::create(lv_obj_t* parent, int x, int y, int w, int h, const Fonts& fonts,
                          const Colors& colors) {
    obj_ = lv_obj_create(parent);
    lv_obj_remove_style_all(obj_);
    lv_obj_set_pos(obj_, x, y);
    lv_obj_set_size(obj_, w, h);
    lv_obj_remove_flag(obj_, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(obj_, draw_cb, LV_EVENT_DRAW_MAIN, this);
    f_ = fonts;
    c_ = colors;
    h_ = h;
}

void FxPickerView::set_visible(bool visible) {
    if (!obj_) return;
    if (visible) lv_obj_remove_flag(obj_, LV_OBJ_FLAG_HIDDEN);
    else lv_obj_add_flag(obj_, LV_OBJ_FLAG_HIDDEN);
}

void FxPickerView::set(const ZtFxPicker& picker, const char* title) {
    if (std::memcmp(&picker, &p_, sizeof p_) == 0 && title == title_) return;
    p_ = picker;
    title_ = title;
    // The row of the chosen command stays in view.
    const int cols = std::max<int>(1, p_.columns);
    const int rows = std::max(1, (h_ - kTitleH - kGap) / (kChipH + kGap));
    const int row = p_.selected / cols;
    if (row < top_row_) top_row_ = row;
    if (row >= top_row_ + rows) top_row_ = row - rows + 1;
    if (obj_) lv_obj_invalidate(obj_);
}

void FxPickerView::draw_cb(lv_event_t* e) {
    auto* self = static_cast<FxPickerView*>(lv_event_get_user_data(e));
    lv_area_t box;
    lv_obj_get_coords(self->obj_, &box);
    self->draw(lv_event_get_layer(e), box);
}

void FxPickerView::draw(lv_layer_t* layer, const lv_area_t& box) const {
    fill(layer, box, lv_color_hex(c_.bg));
    const int lh = lv_font_get_line_height(f_.label);
    const int cols = std::max<int>(1, p_.columns);
    const int count = p_.count;

    // Title: the name, then the chosen command and its number on the right.
    text(layer, {box.x1 + 6, box.y1 + (kTitleH - lh) / 2, box.x2 - 80, box.y1 + kTitleH}, title_, f_.label,
         lv_color_hex(c_.accent));
    if (p_.selected < count) {
        char sel[16];
        std::snprintf(sel, sizeof sel, "%02X %s", p_.ids[p_.selected], p_.names[p_.selected]);
        text(layer, {box.x2 - 80, box.y1 + (kTitleH - lh) / 2, box.x2 - 6, box.y1 + kTitleH}, sel, f_.label,
             lv_color_hex(c_.hi), LV_TEXT_ALIGN_RIGHT);
    }
    fill(layer, {box.x1 + 4, box.y1 + kTitleH - 2, box.x2 - 4, box.y1 + kTitleH - 1}, lv_color_hex(c_.accent));

    // The grid of chips.
    const int gx = box.x1 + kGap;
    const int cw = (lv_area_get_width(&box) - (cols + 1) * kGap) / cols;
    const int rows = std::max(1, (h_ - kTitleH - kGap) / (kChipH + kGap));
    for (int r = 0; r < rows; ++r) {
        for (int c = 0; c < cols; ++c) {
            const int i = (top_row_ + r) * cols + c;
            if (i >= count) break;
            const int x = gx + c * (cw + kGap);
            const int y = box.y1 + kTitleH + kGap + r * (kChipH + kGap);
            const lv_area_t chip{x, y, x + cw - 1, y + kChipH - 1};
            const bool sel = i == p_.selected;
            fill(layer, chip, lv_color_hex(sel ? c_.cursor : c_.panel), 5);
            text(layer, {chip.x1, y + (kChipH - lh) / 2, chip.x2, chip.y2}, p_.names[i], f_.label,
                 lv_color_hex(sel ? c_.on_cursor : c_.fg), LV_TEXT_ALIGN_CENTER);
        }
    }

    // Scroll bar when the grid does not fit.
    const int total_rows = (count + cols - 1) / cols;
    if (total_rows > rows) {
        const int y1 = box.y1 + kTitleH + kGap, y2 = box.y2 - 2;
        const int track = y2 - y1;
        const int thumb = std::max(6, track * rows / total_rows);
        const int ty = y1 + (track - thumb) * top_row_ / std::max(1, total_rows - rows);
        fill(layer, {box.x2 - 2, ty, box.x2 - 1, ty + thumb}, lv_color_hex(c_.dim), 1);
    }
}

} // namespace screen
