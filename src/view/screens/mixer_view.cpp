/*
 * MixerView. See mixer_view.h.
 */
#include "mixer_view.h"

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstring>

namespace screen {
namespace {

constexpr int kStrips = 9;
constexpr int kBadge = 14;       // badge diameter, px
constexpr int kBarW = 9;         // one channel of a meter, px
constexpr int kBarGap = 2;
constexpr int kSegH = 2;         // meter slice and gap, px: a near-continuous gradient
constexpr int kSegGap = 0;
constexpr uint32_t kPeakHoldMs = 1200;

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
    dsc.text_local = 1;
    dsc.font = font;
    dsc.color = color;
    dsc.align = LV_TEXT_ALIGN_CENTER;
    lv_draw_label(layer, &dsc, &a);
}

// Text box `h` px high centred on line y.
lv_area_t line_box(int x1, int x2, int y, const lv_font_t* font) {
    const int h = lv_font_get_line_height(font);
    return {x1, y, x2, y + h - 1};
}

// dB scale -60..0 dBFS to 0..1, as the old meters.
float level_fraction(uint16_t level) {
    if (level == 0) return 0.0f;
    const double db = 20.0 * std::log10(level / 65535.0);
    return static_cast<float>(std::clamp((db + 60.0) / 60.0, 0.0, 1.0));
}

} // namespace

namespace {
MeterScale g_meter_scale{true, 0, 0};
}

void set_meter_scale(const MeterScale& scale) {
    g_meter_scale = scale;
}

lv_color_t meter_color(float fraction) {
    if (!g_meter_scale.colorful) {
        // lv_color_mix(a, b, r): r of a, 255 - r of b.
        const auto r = static_cast<uint8_t>(std::lround(std::clamp(fraction, 0.0f, 1.0f) * 255.0f));
        return lv_color_mix(lv_color_hex(g_meter_scale.high), lv_color_hex(g_meter_scale.low), r);
    }
    // Hue 120 (green) at the bottom down to 0 (red) at full scale; yellow at
    // 60% and orange at 80% of the meter.
    const float f = std::clamp(fraction, 0.0f, 1.0f);
    const float hue = f < 0.6f ? 120.0f - (f / 0.6f) * 60.0f
                    : f < 0.8f ? 60.0f - ((f - 0.6f) / 0.2f) * 30.0f
                               : 30.0f - ((f - 0.8f) / 0.2f) * 30.0f;
    return lv_color_hsv_to_rgb(static_cast<uint16_t>(hue), 85, 95);
}

void MixerView::create(lv_obj_t* parent, int x, int y, int w, int h, const lv_font_t* font,
                       const lv_font_t* small_font, const Colors& colors) {
    obj_ = lv_obj_create(parent);
    lv_obj_remove_style_all(obj_);
    lv_obj_set_pos(obj_, x, y);
    lv_obj_set_size(obj_, w, h);
    lv_obj_remove_flag(obj_, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(obj_, draw_cb, LV_EVENT_DRAW_MAIN, this);
    font_ = font;
    small_ = small_font ? small_font : font;
    c_ = colors;
    w_ = w;
    h_ = h;
    // Meter height: below the badge, above the volume and the M/S row.
    const int meter_h = h - (kBadge + 4) - lv_font_get_line_height(font_) -
                        lv_font_get_line_height(small_) - 4;
    segments_ = std::max(4, meter_h / (kSegH + kSegGap));
    lit_.fill(0);
    peak_.fill(0);
    peak_at_.fill(0);
    valid_ = false;
}

void MixerView::set_visible(bool visible) {
    if (!obj_) return;
    if (visible) lv_obj_remove_flag(obj_, LV_OBJ_FLAG_HIDDEN);
    else lv_obj_add_flag(obj_, LV_OBJ_FLAG_HIDDEN);
}

void MixerView::set(const ZtMixer& mixer, const std::array<uint16_t, 18>& levels) {
    bool changed = !valid_ || std::memcmp(&mixer, &m_, sizeof m_) != 0;
    m_ = mixer;
    const uint32_t now = lv_tick_get();
    for (size_t i = 0; i < levels.size(); ++i) {
        const int lit = static_cast<int>(std::lround(level_fraction(levels[i]) * segments_));
        int peak = peak_[i];
        if (lit >= peak) {
            peak = lit;
            peak_at_[i] = now;
        }
        else if (now - peak_at_[i] > kPeakHoldMs) {
            peak = std::max(lit, peak - 1);
        }
        if (lit != lit_[i] || peak != peak_[i]) {
            lit_[i] = lit;
            peak_[i] = peak;
            changed = true;
        }
    }
    valid_ = true;
    if (changed && obj_) lv_obj_invalidate(obj_);
}

void MixerView::draw_cb(lv_event_t* e) {
    auto* self = static_cast<MixerView*>(lv_event_get_user_data(e));
    lv_area_t box;
    lv_obj_get_coords(self->obj_, &box);
    self->draw(lv_event_get_layer(e), box);
}

void MixerView::draw_meter(lv_layer_t* layer, int x, int y_top, int y_bottom, int lit, int peak,
                           bool dim) const {
    for (int s = 0; s < segments_; ++s) {
        const int y2 = y_bottom - s * (kSegH + kSegGap);
        const int y1 = y2 - kSegH + 1;
        if (y1 < y_top) break;
        const float f = (s + 0.5f) / segments_;
        lv_color_t color = lv_color_hex(c_.unlit);
        if (s < lit || (s == peak - 1 && peak > 0)) {
            color = meter_color(f);
            if (dim) color = lv_color_mix(color, lv_color_hex(c_.unlit), 90);
        }
        fill(layer, {x, y1, x + kBarW - 1, y2}, color);
    }
}

void MixerView::draw(lv_layer_t* layer, const lv_area_t& box) const {
    if (!valid_) return;
    const int col_w = w_ / kStrips;
    const int font_h = lv_font_get_line_height(font_);
    const int small_h = lv_font_get_line_height(small_);
    const int ms_y = box.y2 - small_h;
    const int vol_y = ms_y - font_h - 1;
    const int meter_top = box.y1 + kBadge + 4;
    const int meter_bottom = vol_y - 3;
    for (int s = 0; s < kStrips; ++s) {
        const int x0 = box.x1 + s * col_w;
        const int cx = x0 + col_w / 2;
        const bool selected = m_.selected == s;
        const bool master = s == 8;
        const bool muted = !master && (m_.mute & (1u << s));
        const bool solo = !master && (m_.solo & (1u << s));
        const uint32_t badge_color = selected ? c_.selected : c_.badge;

        // Badge: the track number in a circle, MIX in a pill.
        if (master) {
            const lv_area_t pill{x0 + 2, box.y1 + 1, x0 + col_w - 3, box.y1 + kBadge};
            fill(layer, pill, lv_color_hex(badge_color), LV_RADIUS_CIRCLE);
            lv_area_t t = line_box(pill.x1, pill.x2, box.y1 + 1 + (kBadge - small_h) / 2, small_);
            text(layer, t, "MIX", small_, lv_color_hex(selected ? c_.on_selected : c_.on_badge));
        }
        else {
            const lv_area_t badge{cx - kBadge / 2, box.y1 + 1, cx - kBadge / 2 + kBadge - 1,
                                  box.y1 + kBadge};
            fill(layer, badge, lv_color_hex(badge_color), LV_RADIUS_CIRCLE);
            char num[2] = {static_cast<char>('1' + s), 0};
            lv_area_t t = line_box(badge.x1, badge.x2, box.y1 + 1 + (kBadge - small_h) / 2, small_);
            text(layer, t, num, small_, lv_color_hex(selected ? c_.on_selected : c_.on_badge));
        }

        // Stereo meter.
        const int pair_w = 2 * kBarW + kBarGap;
        const int mx = cx - pair_w / 2;
        draw_meter(layer, mx, meter_top, meter_bottom, lit_[2 * s], peak_[2 * s], muted);
        draw_meter(layer, mx + kBarW + kBarGap, meter_top, meter_bottom, lit_[2 * s + 1],
                   peak_[2 * s + 1], muted);

        // Limiter line across the MIX meter.
        if (master) {
            const int span = meter_bottom - meter_top;
            const int ly = meter_bottom - span * m_.limiter / 255;
            const uint32_t lc = m_.limit_mode ? c_.limiter : c_.hi;
            fill(layer, {mx - 3, ly - 1, mx + pair_w + 2, ly}, lv_color_hex(lc));
        }

        // Volume: on a red pill when selected.
        char vol[3];
        std::snprintf(vol, sizeof vol, "%02X", m_.volumes[s]);
        const lv_area_t vbox = line_box(x0 + 3, x0 + col_w - 4, vol_y, font_);
        if (selected && !m_.limit_mode) {
            fill(layer, vbox, lv_color_hex(c_.selected), LV_RADIUS_CIRCLE);
        }
        text(layer, vbox, vol, font_,
             lv_color_hex(selected && !m_.limit_mode ? c_.on_selected : c_.text));

        // M and S lights (tracks only); on MIX the limiter value.
        if (master) {
            char lim[6];
            std::snprintf(lim, sizeof lim, "L%02X", m_.limiter);
            text(layer, line_box(x0, x0 + col_w - 1, ms_y, small_), lim, small_,
                 lv_color_hex(m_.limit_mode ? c_.limiter : c_.dim));
        }
        else {
            const int half = col_w / 2;
            const lv_area_t mb{x0 + 2, ms_y, x0 + half - 1, ms_y + small_h - 1};
            const lv_area_t sb{x0 + half + 1, ms_y, x0 + col_w - 3, ms_y + small_h - 1};
            if (muted) fill(layer, mb, lv_color_hex(c_.muted), 2);
            if (solo) fill(layer, sb, lv_color_hex(c_.solo), 2);
            text(layer, mb, "M", small_, lv_color_hex(muted ? c_.on_selected : c_.dim));
            text(layer, sb, "S", small_, lv_color_hex(solo ? c_.on_solo : c_.dim));
        }
    }
}

} // namespace screen
