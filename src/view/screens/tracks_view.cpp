/*
 * TracksView. See tracks_view.h.
 */
#include "tracks_view.h"

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <string>

namespace screen {
namespace {

// The browser's glyphs for samples and instruments (Phosphor Fill).
constexpr const char* kIconSample = "";  // waveform
constexpr const char* kIconSynth = "";   // piano-keys
constexpr const char* kKinds[8] = {"", "WAV", "MACRO", "SAMPLER", "FM", "HYPER", "MIDI", "EXT"};

void fill(lv_layer_t* layer, const lv_area_t& a, lv_color_t color, int radius = 0,
          lv_opa_t opa = LV_OPA_COVER) {
    lv_draw_rect_dsc_t dsc;
    lv_draw_rect_dsc_init(&dsc);
    dsc.bg_color = color;
    dsc.bg_opa = opa;
    dsc.radius = radius;
    lv_draw_rect(layer, &dsc, &a);
}

void outline(lv_layer_t* layer, const lv_area_t& a, lv_color_t color, int radius, int width) {
    lv_draw_rect_dsc_t dsc;
    lv_draw_rect_dsc_init(&dsc);
    dsc.bg_opa = LV_OPA_TRANSP;
    dsc.border_color = color;
    dsc.border_width = width;
    dsc.border_opa = LV_OPA_COVER;
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
    dsc.flag = LV_TEXT_FLAG_EXPAND;  // one line: the callers fit the text
    lv_draw_label(layer, &dsc, &a);
}

// `s` cut to fit `width` px on one line, with a trailing '.' when cut.
std::string fit(const char* s, const lv_font_t* font, int width) {
    std::string out = s;
    auto w = [&](const std::string& x) {
        lv_point_t p;
        lv_text_get_size(&p, x.c_str(), font, 0, 0, LV_COORD_MAX, LV_TEXT_FLAG_NONE);
        return p.x;
    };
    if (w(out) <= width) return out;
    while (!out.empty() && w(out + ".") > width) out.pop_back();
    return out + ".";
}

lv_color_t scale(const std::array<uint32_t, 5>& stops, float t) {
    const float p = std::clamp(t, 0.0f, 1.0f) * 4.0f;
    const int i = std::min(static_cast<int>(p), 3);
    const auto mix = static_cast<uint8_t>(std::lround((p - i) * 255.0f));
    return lv_color_mix(lv_color_hex(stops[i + 1]), lv_color_hex(stops[i]), mix);
}

const char* icon_of(uint8_t kind) {
    return kind == 3 ? kIconSample : kIconSynth;
}

} // namespace

void TracksView::create(lv_obj_t* parent, int x, int y, int w, int h, const Fonts& fonts,
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

void TracksView::set_visible(bool visible) {
    if (!obj_) return;
    if (visible) lv_obj_remove_flag(obj_, LV_OBJ_FLAG_HIDDEN);
    else lv_obj_add_flag(obj_, LV_OBJ_FLAG_HIDDEN);
}

void TracksView::set(const ZtTracksPage& page, const std::array<uint16_t, 18>& levels,
                     const std::string& pool_title, const std::string& empty) {
    // Level bars in 2 px steps, -60..0 dBFS, so a still track does not redraw.
    std::array<uint8_t, 8> px{};
    for (int t = 0; t < 8; ++t) {
        const int v = std::max(levels[2 * t], levels[2 * t + 1]);
        if (v > 0) {
            const double db = 20.0 * std::log10(v / 65535.0);
            px[t] = static_cast<uint8_t>(std::clamp((db + 60.0) / 60.0, 0.0, 1.0) * 50) * 2;
        }
    }
    if (std::memcmp(&page, &page_, sizeof page) == 0 && px == level_px_ && pool_title == pool_title_) return;
    page_ = page;
    level_px_ = px;
    pool_title_ = pool_title;
    empty_ = empty;
    if (obj_) lv_obj_invalidate(obj_);
}

void TracksView::draw_cb(lv_event_t* e) {
    auto* self = static_cast<TracksView*>(lv_event_get_user_data(e));
    lv_area_t box;
    lv_obj_get_coords(self->obj_, &box);
    self->draw(lv_event_get_layer(e), box);
}

void TracksView::draw(lv_layer_t* layer, const lv_area_t& box) const {
    fill(layer, box, lv_color_hex(c_.bg));
    if (page_.cursor < 8) draw_tracks(layer, box);
    else draw_pool(layer, box);
}

void TracksView::draw_tracks(lv_layer_t* layer, const lv_area_t& box) const {
    const int gap = 4;
    const int cw = (lv_area_get_width(&box) - 5 * gap) / 4;
    const int ch = (lv_area_get_height(&box) - 3 * gap) / 2;
    const int ih = lv_font_get_line_height(f_.icon);
    const int lh = lv_font_get_line_height(f_.label);
    const int sh = lv_font_get_line_height(f_.small);
    for (int t = 0; t < 8; ++t) {
        const ZtTrackRow& r = page_.tracks[t];
        const int x1 = box.x1 + gap + (t % 4) * (cw + gap);
        const int y1 = box.y1 + gap + (t / 4) * (ch + gap);
        const lv_area_t card{x1, y1, x1 + cw - 1, y1 + ch - 1};
        const bool sel = page_.cursor == t;
        const bool muted = r.flags & 1, solo = r.flags & 2, playing = r.flags & 4;
        fill(layer, card, lv_color_hex(c_.panel), 6);
        if (sel) outline(layer, card, lv_color_hex(page_.editing ? c_.alert : c_.cursor), 6, 2);

        // Number on a badge: muted, soloed, playing.
        char num[4];
        std::snprintf(num, sizeof num, "%d", t + 1);
        const uint32_t badge = muted ? c_.muted : solo ? c_.solo : playing ? c_.playing : c_.badge;
        const uint32_t on = muted ? c_.on_badge : solo ? c_.on_solo : playing ? c_.bg : c_.on_badge;
        const lv_area_t b{card.x1 + 4, card.y1 + 4, card.x1 + 4 + lh, card.y1 + 4 + lh - 1};
        fill(layer, b, lv_color_hex(badge), 4);
        text(layer, b, num, f_.label, lv_color_hex(on), LV_TEXT_ALIGN_CENTER);

        // Kind icon and slot on the right of the badge.
        if (r.instrument >= 0) {
            text(layer, {b.x2 + 4, card.y1 + 4 + (lh - ih) / 2, b.x2 + 22, card.y1 + 4 + (lh + ih) / 2}, icon_of(r.kind),
                 f_.icon, lv_color_hex(muted ? c_.dim : c_.accent), LV_TEXT_ALIGN_CENTER);
            // While it plays, the note on the right (bright); else the slot.
            char slot[8];
            if (r.note[0]) std::snprintf(slot, sizeof slot, "%s", r.note);
            else std::snprintf(slot, sizeof slot, "%02X", r.instrument);
            text(layer, {card.x2 - 40, card.y1 + 4, card.x2 - 4, card.y1 + 4 + lh}, slot, f_.label,
                 lv_color_hex(muted ? c_.dim : r.note[0] ? c_.playing : c_.hi), LV_TEXT_ALIGN_RIGHT);
            // Name (or the kind when unnamed), then the detail.
            // While it plays the note takes the slot's place: the slot moves
            // before the name.
            const char* base = r.name[0] ? r.name : kKinds[std::min<int>(r.kind, 7)];
            char named[24];
            if (r.note[0]) std::snprintf(named, sizeof named, "%02X %s", r.instrument, base);
            else std::snprintf(named, sizeof named, "%s", base);
            const char* name = named;
            const int ny = card.y1 + 6 + lh;
            const int w = cw - 10;
            text(layer, {card.x1 + 5, ny, card.x2 - 4, ny + sh}, fit(name, f_.small, w).c_str(), f_.small,
                 lv_color_hex(muted ? c_.dim : c_.fg));
            // The detail when it fits above the level bar (large APP font
            // sizes leave room for the name only).
            if (ny + 2 * sh - 4 <= card.y2 - 6) {
                text(layer, {card.x1 + 5, ny + sh, card.x2 - 4, ny + 2 * sh}, fit(r.detail, f_.small, w).c_str(),
                     f_.small, lv_color_hex(c_.fg));
            }
        }
        else {
            text(layer, {card.x1 + 5, card.y1 + 6 + lh, card.x2 - 4, card.y1 + 6 + lh + sh},
                 fit(empty_.c_str(), f_.small, cw - 10).c_str(), f_.small, lv_color_hex(c_.dim));
        }

        // Live level along the bottom.
        const int bar_y = card.y2 - 6;
        const lv_area_t track{card.x1 + 5, bar_y, card.x2 - 5, bar_y + 2};
        fill(layer, track, lv_color_hex(c_.bg), 1);
        const int len = (lv_area_get_width(&track) * level_px_[t]) / 100;
        if (len > 0 && !muted) {
            fill(layer, {track.x1, track.y1, track.x1 + len - 1, track.y2}, scale(c_.chain, level_px_[t] / 100.0f), 1);
        }
    }
}

void TracksView::draw_pool(lv_layer_t* layer, const lv_area_t& box) const {
    const int sh = lv_font_get_line_height(f_.small);
    const int ih = lv_font_get_line_height(f_.icon);
    const int head = sh + 4;
    const int rh = std::max(sh + 4, (lv_area_get_height(&box) - head) / ZT_POOL_ROWS);
    // Column layout: icon, slot, name, four bars, EQ.
    const int x_icon = box.x1 + 4, x_slot = box.x1 + 24, x_name = box.x1 + 46;
    const int x_bars = box.x1 + 148, bar_w = 30, bar_gap = 6;
    const int x_eq = x_bars + 4 * (bar_w + bar_gap) + 2;
    static const char* const kSends[4] = {"DRY", "MFX", "DEL", "REV"};

    // Header: the title, then the column names over the bars.
    text(layer, {x_icon, box.y1 + 2, x_bars - 4, box.y1 + 2 + sh}, pool_title_.c_str(), f_.small,
         lv_color_hex(c_.accent));
    for (int k = 0; k < 4; ++k) {
        const int x = x_bars + k * (bar_w + bar_gap);
        const bool col = page_.column == k + 1;
        text(layer, {x, box.y1 + 2, x + bar_w, box.y1 + 2 + sh}, kSends[k], f_.small,
             lv_color_hex(col ? c_.hi : c_.dim), LV_TEXT_ALIGN_CENTER);
    }
    text(layer, {x_eq, box.y1 + 2, box.x2 - 2, box.y1 + 2 + sh}, "EQ", f_.small,
         lv_color_hex(page_.column == 5 ? c_.hi : c_.dim), LV_TEXT_ALIGN_CENTER);

    for (int i = 0; i < ZT_POOL_ROWS; ++i) {
        const ZtTrackRow& r = page_.pool[i];
        const int slot = page_.pool_first + i;
        const int y = box.y1 + head + i * rh;
        const int mid = y + rh / 2;
        const bool sel_row = page_.cursor == 8 + slot;
        if (i % 2 == 1) fill(layer, {box.x1, y, box.x2, y + rh - 1}, lv_color_hex(c_.panel));
        const bool empty = r.instrument < 0 || r.kind == 0;
        const uint32_t fg = empty ? c_.dim : c_.fg;
        // NAME cell (column 0) on a pill when selected.
        if (sel_row && page_.column == 0) {
            fill(layer, {x_icon - 2, y + 1, x_bars - 6, y + rh - 2}, lv_color_hex(page_.editing ? c_.alert : c_.cursor), 5);
        }
        const bool on_pill = sel_row && page_.column == 0;
        const uint32_t pfg = on_pill ? c_.on_cursor : fg;
        if (!empty) {
            text(layer, {x_icon, mid - ih / 2, x_icon + 18, mid - ih / 2 + ih - 1}, icon_of(r.kind), f_.icon,
                 lv_color_hex(on_pill ? c_.on_cursor : c_.accent), LV_TEXT_ALIGN_CENTER);
        }
        char hex[4];
        std::snprintf(hex, sizeof hex, "%02X", slot);
        text(layer, {x_slot, mid - sh / 2, x_slot + 20, mid + sh / 2}, hex, f_.small, lv_color_hex(on_pill ? c_.on_cursor : c_.hi));
        const char* name = empty ? "--" : r.name[0] ? r.name : kKinds[std::min<int>(r.kind, 7)];
        text(layer, {x_name, mid - sh / 2, x_bars - 8, mid + sh / 2}, fit(name, f_.small, x_bars - 10 - x_name).c_str(),
             f_.small, lv_color_hex(pfg));
        if (empty) continue;
        for (int k = 0; k < 4; ++k) {
            const int x = x_bars + k * (bar_w + bar_gap);
            const bool cell = sel_row && page_.column == k + 1;
            if (cell) fill(layer, {x - 3, y + 1, x + bar_w + 2, y + rh - 2}, lv_color_hex(page_.editing ? c_.alert : c_.cursor), 4);
            const lv_area_t tr{x, mid - 2, x + bar_w - 1, mid + 2};
            fill(layer, tr, lv_color_hex(cell ? c_.on_cursor : c_.bg), 2);
            const int len = (bar_w * r.sends[k] + 127) / 255;
            if (len > 0) fill(layer, {tr.x1, tr.y1, tr.x1 + len - 1, tr.y2}, scale(c_.chain, r.sends[k] / 255.0f), 2);
        }
        const bool eq_cell = sel_row && page_.column == 5;
        if (eq_cell) fill(layer, {x_eq, y + 1, box.x2 - 2, y + rh - 2}, lv_color_hex(page_.editing ? c_.alert : c_.cursor), 4);
        char eq[4];
        if (r.eq >= 0x80) std::snprintf(eq, sizeof eq, "--");
        else std::snprintf(eq, sizeof eq, "%02X", r.eq);
        text(layer, {x_eq, mid - sh / 2, box.x2 - 2, mid + sh / 2}, eq, f_.small,
             lv_color_hex(eq_cell ? c_.on_cursor : r.eq >= 0x80 ? c_.dim : c_.fg), LV_TEXT_ALIGN_CENTER);
    }
}

} // namespace screen
