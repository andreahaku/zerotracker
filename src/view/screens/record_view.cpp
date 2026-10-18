/*
 * RecordView. See record_view.h.
 */
#include "record_view.h"

#include <algorithm>
#include <cmath>
#include <cstdio>

namespace screen {
namespace {

constexpr const char* kIconMic = "";  // microphone (Phosphor Fill)

void fill(lv_layer_t* layer, const lv_area_t& a, lv_color_t color, int radius = 0,
          lv_opa_t opa = LV_OPA_COVER) {
    lv_draw_rect_dsc_t dsc;
    lv_draw_rect_dsc_init(&dsc);
    dsc.bg_color = color;
    dsc.bg_opa = opa;
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

void RecordView::create(lv_obj_t* parent, int x, int y, int w, int h, const Fonts& fonts,
                        const Colors& colors) {
    obj_ = lv_obj_create(parent);
    lv_obj_remove_style_all(obj_);
    lv_obj_set_pos(obj_, x, y);
    lv_obj_set_size(obj_, w, h);
    lv_obj_remove_flag(obj_, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(obj_, draw_cb, LV_EVENT_DRAW_MAIN, this);
    f_ = fonts;
    c_ = colors;
}

void RecordView::set_visible(bool visible) {
    if (!obj_) return;
    if (visible) lv_obj_remove_flag(obj_, LV_OBJ_FLAG_HIDDEN);
    else lv_obj_add_flag(obj_, LV_OBJ_FLAG_HIDDEN);
}

void RecordView::set(State state) {
    s_ = std::move(state);
    if (obj_) lv_obj_invalidate(obj_);
}

void RecordView::draw_cb(lv_event_t* e) {
    auto* self = static_cast<RecordView*>(lv_event_get_user_data(e));
    lv_area_t box;
    lv_obj_get_coords(self->obj_, &box);
    self->draw(lv_event_get_layer(e), box);
}

void RecordView::draw(lv_layer_t* layer, const lv_area_t& box) const {
    fill(layer, box, lv_color_hex(c_.bg));
    const int ih = lv_font_get_line_height(f_.icon);
    const int lh = lv_font_get_line_height(f_.label);
    const int sh = lv_font_get_line_height(f_.small);
    const int bh = lv_font_get_line_height(f_.big);

    // Title row: the microphone (red while recording) and the title.
    const uint32_t mic = s_.recording ? c_.alert : c_.accent;
    text(layer, {box.x1 + 6, box.y1 + 4, box.x1 + 26, box.y1 + 4 + ih}, kIconMic, f_.icon, lv_color_hex(mic),
         LV_TEXT_ALIGN_CENTER);
    text(layer, {box.x1 + 30, box.y1 + 4 + (ih - lh) / 2, box.x2 - 6, box.y1 + 4 + ih}, s_.title.c_str(), f_.label,
         lv_color_hex(c_.hi));

    // The time, large, and the status beside it.
    char t[16];
    const int tenths = static_cast<int>(s_.seconds * 10);
    std::snprintf(t, sizeof t, "%d:%02d.%d", tenths / 600, tenths / 10 % 60, tenths % 10);
    const int ty = box.y1 + 6 + ih;
    text(layer, {box.x1 + 8, ty, box.x1 + 170, ty + bh}, t, f_.big, lv_color_hex(s_.recording ? c_.hi : c_.fg));
    const std::string& status = s_.error.empty() ? s_.status : s_.error;
    text(layer, {box.x1 + 176, ty + (bh - lh) / 2, box.x2 - 8, ty + (bh + lh) / 2}, status.c_str(), f_.label,
         lv_color_hex(!s_.error.empty() || s_.recording ? c_.alert : c_.dim), LV_TEXT_ALIGN_RIGHT);

    // Level meter.
    const int my = ty + bh + 4;
    const lv_area_t meter{box.x1 + 8, my, box.x2 - 8, my + 5};
    fill(layer, meter, lv_color_hex(c_.panel), 2);
    const int len = static_cast<int>(lv_area_get_width(&meter) * std::clamp(s_.level, 0.0f, 1.0f));
    if (len > 0) {
        const uint32_t color = s_.level > 0.9f ? c_.alert : c_.accent;
        fill(layer, {meter.x1, meter.y1, meter.x1 + len - 1, meter.y2}, lv_color_hex(color), 2);
    }

    // The waveform as it comes in, one column per peak, newest on the right.
    const int wy1 = my + 10, wy2 = box.y2 - sh - 6;
    const lv_area_t wave{box.x1 + 8, wy1, box.x2 - 8, wy2};
    fill(layer, wave, lv_color_hex(c_.panel), 4);
    const int cols = lv_area_get_width(&wave) - 4;
    const int mid = (wave.y1 + wave.y2) / 2, half = (wave.y2 - wave.y1) / 2 - 2;
    const int n = std::min<int>(cols, static_cast<int>(s_.peaks.size()));
    for (int i = 0; i < n; ++i) {
        const float p = s_.peaks[s_.peaks.size() - n + i];
        const int hgt = std::max(1, static_cast<int>(p * half));
        const int x = wave.x2 - 2 - (n - 1 - i);
        fill(layer, {x, mid - hgt, x, mid + hgt}, lv_color_hex(c_.accent));
    }

    text(layer, {box.x1 + 8, box.y2 - sh - 2, box.x2 - 8, box.y2}, s_.hint.c_str(), f_.small, lv_color_hex(c_.fg));
}

} // namespace screen
