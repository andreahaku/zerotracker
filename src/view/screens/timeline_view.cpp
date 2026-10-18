/*
 * TimelineView. See timeline_view.h.
 */
#include "timeline_view.h"

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <iterator>

namespace screen {
namespace {

// Chain colour: the theme's scale (COLOR: light green
// through green and teal to blue and indigo), interpolated smoothly from 00
// to 8F (higher chains stay on the last stop). Songs usually give track N the
// chains N0-NF, so each track's chains get their own shade of the scale.
lv_color_t chain_color(const std::array<uint32_t, 5>& kChainScale, uint8_t chain, bool dim) {
    constexpr int kStops = 5;
    const double t = std::min(chain, uint8_t{0x8F}) / double{0x8F} * (kStops - 1);
    const int i = std::min(static_cast<int>(t), kStops - 2);
    const auto mix = static_cast<uint8_t>(std::lround((t - i) * 255.0));
    // lv_color_mix(a, b, r): r of a, 255 - r of b.
    const lv_color_t c = lv_color_mix(lv_color_hex(kChainScale[i + 1]), lv_color_hex(kChainScale[i]), mix);
    return dim ? lv_color_mix(c, lv_color_black(), 110) : c;
}

void fill(lv_layer_t* layer, const lv_area_t& a, lv_color_t color, int radius = 0) {
    lv_draw_rect_dsc_t dsc;
    lv_draw_rect_dsc_init(&dsc);
    dsc.bg_color = color;
    dsc.bg_opa = LV_OPA_COVER;
    dsc.radius = radius;
    lv_draw_rect(layer, &dsc, &a);
}

void text(lv_layer_t* layer, const lv_area_t& a, const char* s, const lv_font_t* font,
          lv_color_t color) {
    lv_draw_label_dsc_t dsc;
    lv_draw_label_dsc_init(&dsc);
    dsc.text = s;
    dsc.text_local = 1;  // drawing is deferred: LVGL keeps its own copy
    dsc.font = font;
    dsc.color = color;
    dsc.align = LV_TEXT_ALIGN_CENTER;
    lv_draw_label(layer, &dsc, &a);
}

} // namespace

void TimelineView::create(lv_obj_t* parent, int x, int y, int w, int h, const lv_font_t* font,
                          const lv_font_t* badge_font, const Colors& colors) {
    obj_ = lv_obj_create(parent);
    lv_obj_remove_style_all(obj_);
    lv_obj_set_pos(obj_, x, y);
    lv_obj_set_size(obj_, w, h);
    lv_obj_remove_flag(obj_, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(obj_, draw_cb, LV_EVENT_DRAW_MAIN, this);
    font_ = font;
    badge_font_ = badge_font ? badge_font : font;
    c_ = colors;
    w_ = w;
    h_ = h;
    lane_h_ = std::max(8, h / 8);
    glyph_w_ = std::max<int>(1, lv_font_get_glyph_width(font_, '0', 0));
    // Two hex digits and a little air each side.
    cell_w_ = 2 * glyph_w_ + 8;
    cells_x_ = lane_h_ + 4;
    valid_ = false;
}

void TimelineView::set_visible(bool visible) {
    if (!obj_) return;
    if (visible) lv_obj_remove_flag(obj_, LV_OBJ_FLAG_HIDDEN);
    else lv_obj_add_flag(obj_, LV_OBJ_FLAG_HIDDEN);
}

int TimelineView::visible_rows() const {
    return std::clamp((w_ - cells_x_) / cell_w_, 1, ZT_TIMELINE_MAX);
}

void TimelineView::set(const ZtTimeline& t) {
    if (valid_ && std::memcmp(&t, &t_, sizeof t) == 0) return;
    t_ = t;
    valid_ = true;
    if (obj_) lv_obj_invalidate(obj_);
}

void TimelineView::draw_cb(lv_event_t* e) {
    auto* self = static_cast<TimelineView*>(lv_event_get_user_data(e));
    lv_area_t box;
    lv_obj_get_coords(self->obj_, &box);
    self->draw(lv_event_get_layer(e), box);
}

void TimelineView::draw(lv_layer_t* layer, const lv_area_t& box) const {
    if (!valid_) return;
    const int font_h = lv_font_get_line_height(font_);
    const int badge_h = lv_font_get_line_height(badge_font_);
    for (int track = 0; track < 8; ++track) {
        const int y0 = box.y1 + track * lane_h_;
        const bool muted = t_.mute & (1u << track);
        const bool solo = t_.solo & (1u << track);

        // Lanes alternate black and dark grey instead of separator lines.
        if (track % 2 == 1) {
            fill(layer, {box.x1, y0, box.x2, y0 + lane_h_ - 1}, lv_color_hex(c_.lane));
        }

        // Badge: a filled circle with the track number.
        const int d = lane_h_ - 3;
        const lv_area_t badge{box.x1 + 2, y0 + 1, box.x1 + 2 + d - 1, y0 + d};
        const uint32_t badge_color = muted ? c_.muted : solo ? c_.solo : c_.badge;
        fill(layer, badge, lv_color_hex(badge_color), LV_RADIUS_CIRCLE);
        char num[2] = {static_cast<char>('1' + track), 0};
        lv_area_t num_area = badge;
        num_area.y1 = y0 + 1 + (d - badge_h) / 2;
        num_area.y2 = num_area.y1 + badge_h - 1;
        text(layer, num_area, num, badge_font_, lv_color_hex(c_.badge_text));

        for (int i = 0; i < t_.count; ++i) {
            const int row = t_.first_row + i;
            const int x0 = box.x1 + cells_x_ + i * cell_w_;
            const int text_w = 2 * glyph_w_;
            const lv_area_t cell{x0, y0 + 1, x0 + text_w + 5, y0 + lane_h_ - 3};
            const bool selected = t_.sel[0] >= 0 && row >= t_.sel[0] && row <= t_.sel[1] &&
                                  track >= t_.sel[2] && track <= t_.sel[3];
            // Playing chain: a filled red pill; cursor: a filled blue box (a
            // blue ring when it sits on the playing chain). Text white on both.
            const bool playing = t_.playing[track] == row;
            const bool cursor = row == t_.cursor_row && track == t_.cursor_track;
            if (selected) fill(layer, cell, lv_color_hex(c_.selection), 3);
            if (playing) {
                fill(layer, cell, lv_color_hex(c_.playing), LV_RADIUS_CIRCLE);
            }
            else if (cursor) {
                fill(layer, cell, lv_color_hex(c_.cursor), 3);
            }
            if (playing && cursor) {
                lv_draw_rect_dsc_t ring;
                lv_draw_rect_dsc_init(&ring);
                ring.bg_opa = LV_OPA_TRANSP;
                ring.border_color = lv_color_hex(c_.cursor);
                ring.border_width = 2;
                ring.border_opa = LV_OPA_COVER;
                ring.radius = LV_RADIUS_CIRCLE;
                lv_draw_rect(layer, &ring, &cell);
            }
            const uint8_t chain = t_.cells[track * ZT_TIMELINE_MAX + i];
            if (chain != 0xFF) {
                char hex[3];
                std::snprintf(hex, sizeof hex, "%02X", chain);
                lv_area_t ta = cell;
                ta.y1 = y0 + (lane_h_ - 1 - font_h) / 2;
                ta.y2 = ta.y1 + font_h - 1;
                const lv_color_t color =
                    playing  ? lv_color_hex(c_.on_playing)
                    : cursor ? lv_color_hex(c_.on_cursor)
                             : chain_color(c_.chain, chain, muted);
                text(layer, ta, hex, font_, color);
            }
        }
    }
}

} // namespace screen
