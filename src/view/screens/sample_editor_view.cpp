/*
 * SampleEditorView. See sample_editor_view.h.
 */
#include "sample_editor_view.h"

#include <algorithm>
#include <cmath>
#include <cstdio>

namespace screen {
namespace {

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

void SampleEditorView::create(lv_obj_t* parent, int x, int y, int w, int h, const lv_font_t* font,
                              const lv_font_t* bold, const lv_font_t* small, const Colors& colors) {
    obj_ = lv_obj_create(parent);
    lv_obj_remove_style_all(obj_);
    lv_obj_set_pos(obj_, x, y);
    lv_obj_set_size(obj_, w, h);
    lv_obj_remove_flag(obj_, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(obj_, draw_cb, LV_EVENT_DRAW_MAIN, this);
    font_ = font;
    bold_ = bold ? bold : font;
    small_ = small ? small : font;
    c_ = colors;
    w_ = w;
    h_ = h;
    // The waveform's top and bottom, as draw() lays them out.
    const int bh = lv_font_get_line_height(bold_);
    const int sh = lv_font_get_line_height(small_);
    wave_top_ = bh + sh + 2;
    wave_bottom_ = h - 1 - sh - 8;
    playhead_ = lv_obj_create(obj_);
    lv_obj_remove_style_all(playhead_);
    lv_obj_set_size(playhead_, 2, wave_bottom_ - wave_top_ + 1);
    lv_obj_set_style_bg_color(playhead_, lv_color_hex(c_.playhead), 0);
    lv_obj_set_style_bg_opa(playhead_, LV_OPA_COVER, 0);
    lv_obj_add_flag(playhead_, LV_OBJ_FLAG_HIDDEN);
}

void SampleEditorView::set_playhead(int64_t frame) {
    if (!playhead_) return;
    const double span = std::max<double>(1.0, double(s_.view_to) - double(s_.view_from));
    const int x = kMargin + static_cast<int>(std::lround((double(frame) - s_.view_from) / span * columns()));
    if (frame < 0 || x < kMargin || x >= kMargin + columns()) {
        lv_obj_add_flag(playhead_, LV_OBJ_FLAG_HIDDEN);
        return;
    }
    lv_obj_set_pos(playhead_, x, wave_top_);
    lv_obj_remove_flag(playhead_, LV_OBJ_FLAG_HIDDEN);
}

void SampleEditorView::set_visible(bool visible) {
    if (!obj_) return;
    if (visible) lv_obj_remove_flag(obj_, LV_OBJ_FLAG_HIDDEN);
    else lv_obj_add_flag(obj_, LV_OBJ_FLAG_HIDDEN);
}

bool SampleEditorView::visible() const {
    return obj_ && !lv_obj_has_flag(obj_, LV_OBJ_FLAG_HIDDEN);
}

void SampleEditorView::set(State state) {
    s_ = std::move(state);
    if (obj_) lv_obj_invalidate(obj_);
}

void SampleEditorView::draw_cb(lv_event_t* e) {
    auto* self = static_cast<SampleEditorView*>(lv_event_get_user_data(e));
    lv_area_t box;
    lv_obj_get_coords(self->obj_, &box);
    self->draw(lv_event_get_layer(e), box);
}

void SampleEditorView::draw(lv_layer_t* layer, const lv_area_t& box) const {
    fill(layer, box, lv_color_hex(c_.bg));
    const int bh = lv_font_get_line_height(bold_);
    const int sh = lv_font_get_line_height(small_);

    // Header: file name, the mode on a pill at the right, the format below.
    lv_point_t mode_size;
    lv_text_get_size(&mode_size, s_.mode.c_str(), small_, 0, 0, LV_COORD_MAX, LV_TEXT_FLAG_NONE);
    const lv_area_t pill{box.x2 - mode_size.x - 12, box.y1 + 2, box.x2 - 2, box.y1 + 2 + sh + 2};
    fill(layer, pill, lv_color_hex(c_.accent), 4);
    text(layer, {pill.x1, pill.y1 + 1, pill.x2, pill.y2}, s_.mode.c_str(), small_, lv_color_hex(c_.bg),
         LV_TEXT_ALIGN_CENTER);
    text(layer, {box.x1 + 4, box.y1 + 1, pill.x1 - 4, box.y1 + bh}, s_.title.c_str(), bold_,
         lv_color_hex(c_.hi));
    const int fy = box.y1 + bh;
    text(layer, {box.x1 + 4, fy, box.x2, fy + sh}, s_.format.c_str(), small_, lv_color_hex(c_.dim));

    // Waveform area.
    const int top = fy + sh + 2;
    const int bottom = box.y2 - sh - 8;
    const int x0 = box.x1 + kMargin;
    const int cols = columns();
    const lv_area_t wave_box{x0, top, x0 + cols - 1, bottom};
    fill(layer, wave_box, lv_color_hex(c_.panel), 3);
    const double span = std::max<double>(1.0, double(s_.view_to) - double(s_.view_from));
    auto x_of = [&](uint32_t frame) {
        return x0 + static_cast<int>(std::lround((double(frame) - s_.view_from) / span * cols));
    };
    auto shade = [&](uint32_t a, uint32_t b, uint32_t color, lv_opa_t opa) {
        const int xa = std::clamp(x_of(a), x0, x0 + cols - 1);
        const int xb = std::clamp(x_of(b), x0, x0 + cols - 1);
        if (xb > xa) fill(layer, {xa, top, xb, bottom}, lv_color_hex(color), 0, opa);
    };
    if (s_.sel_b > s_.sel_a) shade(s_.sel_a, s_.sel_b, c_.selection, LV_OPA_COVER);

    // The wave: one min/max line per column, about the middle.
    const int mid = (top + bottom) / 2;
    const int amp = (bottom - top) / 2 - 1;
    for (int c = 0; c < cols && c < static_cast<int>(s_.lo.size()); ++c) {
        const int y1 = mid - static_cast<int>(std::lround(std::clamp(s_.hi[c], -1.0f, 1.0f) * amp));
        const int y2 = mid - static_cast<int>(std::lround(std::clamp(s_.lo[c], -1.0f, 1.0f) * amp));
        fill(layer, {x0 + c, std::min(y1, y2), x0 + c, std::max(y1, y2)}, lv_color_hex(c_.wave));
    }
    fill(layer, {x0, mid, x0 + cols - 1, mid}, lv_color_hex(c_.dim), 0, LV_OPA_40);

    // Loop: a bracket along the top of the wave.
    if (s_.has_loop) {
        const int xa = x_of(s_.loop_a), xb = x_of(s_.loop_b);
        if (xb >= x0 && xa <= x0 + cols - 1) {
            const int a = std::clamp(xa, x0, x0 + cols - 1), b = std::clamp(xb, x0, x0 + cols - 1);
            fill(layer, {a, top, b, top + 2}, lv_color_hex(c_.loop));
            if (xa >= x0) fill(layer, {xa, top, xa + 1, bottom}, lv_color_hex(c_.loop), 0, LV_OPA_70);
            if (xb <= x0 + cols - 1) fill(layer, {xb - 1, top, xb, bottom}, lv_color_hex(c_.loop), 0, LV_OPA_70);
        }
    }

    // Slices: thin lines with their number at the bottom of the wave.
    for (size_t i = 0; i < s_.slices.size(); ++i) {
        const int x = x_of(s_.slices[i]);
        if (x < x0 || x > x0 + cols - 1) continue;
        fill(layer, {x, top, x, bottom}, lv_color_hex(c_.slice), 0, LV_OPA_80);
        char n[4];
        std::snprintf(n, sizeof n, "%zX", i);
        text(layer, {x + 2, bottom - sh, x + 20, bottom}, n, small_, lv_color_hex(c_.slice));
    }

    // Active marker: a bright line with a small flag; then the playhead.
    if (s_.marker >= 0) {
        const int x = x_of(static_cast<uint32_t>(s_.marker));
        if (x >= x0 && x <= x0 + cols - 1) {
            fill(layer, {x, top, x + 1, bottom}, lv_color_hex(c_.hi));
            fill(layer, {x - 3, top, x + 4, top + 4}, lv_color_hex(c_.hi), 1);
        }
    }

    // Overview: the whole sample as a strip, the visible part in it.
    const int oy = bottom + 3;
    fill(layer, {x0, oy, x0 + cols - 1, oy + 2}, lv_color_hex(c_.panel), 1);
    if (s_.frames > 0) {
        const int a = x0 + static_cast<int>(double(s_.view_from) / s_.frames * cols);
        const int b = x0 + static_cast<int>(double(s_.view_to) / s_.frames * cols);
        fill(layer, {a, oy, std::max(a + 2, b), oy + 2}, lv_color_hex(c_.accent), 1);
    }

    // Footer: positions.
    text(layer, {box.x1 + 4, box.y2 - sh, box.x2 - 4, box.y2}, s_.footer.c_str(), small_,
         lv_color_hex(c_.fg));
}

} // namespace screen
