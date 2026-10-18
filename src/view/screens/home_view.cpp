/*
 * HomeView. See home_view.h.
 */
#include "home_view.h"

#include <algorithm>
#include <cmath>

namespace screen {
namespace {

// Logo tile and menu geometry (px), for the 320 x 144 area above the bar.
constexpr int kTile = 96;
constexpr int kTileX = 12;
constexpr int kTileY = 14;
constexpr int kMenuX = 122;
constexpr int kMenuY = 60;
constexpr int kRowH = 21;

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

// One text line of `font` with its top at y.
lv_area_t line(int x1, int x2, int y, const lv_font_t* font) {
    return {x1, y, x2, y + lv_font_get_line_height(font) - 1};
}

lv_color_t scale(const std::array<uint32_t, 5>& stops, float t) {
    const float p = std::clamp(t, 0.0f, 1.0f) * 4.0f;
    const int i = std::min(static_cast<int>(p), 3);
    const auto mix = static_cast<uint8_t>(std::lround((p - i) * 255.0f));
    // lv_color_mix(a, b, r): r of a, 255 - r of b.
    return lv_color_mix(lv_color_hex(stops[i + 1]), lv_color_hex(stops[i]), mix);
}

} // namespace

void HomeView::create(lv_obj_t* parent, int x, int y, int w, int h, const Fonts& fonts,
                      const Colors& colors, const char* version) {
    obj_ = lv_obj_create(parent);
    lv_obj_remove_style_all(obj_);
    lv_obj_set_pos(obj_, x, y);
    lv_obj_set_size(obj_, w, h);
    lv_obj_remove_flag(obj_, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(obj_, draw_cb, LV_EVENT_DRAW_MAIN, this);
    f_ = fonts;
    c_ = colors;
    version_ = version;
    w_ = w;
    h_ = h;
    bars_.fill(0.5f);
}

void HomeView::set_tagline(const char* tagline) {
    tagline_ = tagline;
    if (obj_) lv_obj_invalidate(obj_);
}

void HomeView::set_visible(bool visible) {
    if (!obj_) return;
    if (visible) lv_obj_remove_flag(obj_, LV_OBJ_FLAG_HIDDEN);
    else lv_obj_add_flag(obj_, LV_OBJ_FLAG_HIDDEN);
}

bool HomeView::visible() const {
    return obj_ && !lv_obj_has_flag(obj_, LV_OBJ_FLAG_HIDDEN);
}

void HomeView::set(const std::array<Item, 4>& items, int selected) {
    bool same = selected == selected_;
    for (size_t i = 0; same && i < items.size(); ++i) {
        same = items[i].icon == items_[i].icon && items[i].label == items_[i].label &&
               items[i].detail == items_[i].detail && items[i].enabled == items_[i].enabled;
    }
    if (same) return;
    items_ = items;
    selected_ = selected;
    if (obj_) lv_obj_invalidate(obj_);
}

void HomeView::animate(uint32_t now_ms) {
    if (!visible()) return;
    // Each bar is two slow sines at its own rates: a calm, never-repeating
    // pattern, like a track meter at rest.
    const float t = static_cast<float>(now_ms) / 1000.0f;
    for (size_t i = 0; i < bars_.size(); ++i) {
        const float k = static_cast<float>(i);
        const float v = 0.5f + 0.32f * std::sin(t * (1.3f + 0.21f * k) + k * 0.9f) +
                        0.18f * std::sin(t * (2.9f - 0.17f * k) + k * 2.3f);
        bars_[i] = std::clamp(v, 0.08f, 1.0f);
    }
    lv_area_t box;
    lv_obj_get_coords(obj_, &box);
    lv_area_t logo{box.x1 + kTileX, box.y1 + kTileY, box.x1 + kTileX + kTile - 1,
                   box.y1 + kTileY + kTile - 1};
    lv_obj_invalidate_area(obj_, &logo);
}

void HomeView::draw_cb(lv_event_t* e) {
    auto* self = static_cast<HomeView*>(lv_event_get_user_data(e));
    lv_area_t box;
    lv_obj_get_coords(self->obj_, &box);
    self->draw(lv_event_get_layer(e), box);
}

void HomeView::draw_logo(lv_layer_t* layer, int x, int y) const {
    // Tile: a rounded square, lit from the top.
    lv_draw_rect_dsc_t tile;
    lv_draw_rect_dsc_init(&tile);
    tile.radius = 22;
    tile.bg_opa = LV_OPA_COVER;
    tile.bg_grad.dir = LV_GRAD_DIR_VER;
    tile.bg_grad.stops_count = 2;
    tile.bg_grad.stops[0].color = lv_color_mix(lv_color_hex(c_.chain[4]), lv_color_hex(c_.panel), 70);
    tile.bg_grad.stops[0].opa = LV_OPA_COVER;
    tile.bg_grad.stops[0].frac = 0;
    tile.bg_grad.stops[1].color = lv_color_hex(c_.panel);
    tile.bg_grad.stops[1].opa = LV_OPA_COVER;
    tile.bg_grad.stops[1].frac = 255;
    tile.border_color = lv_color_mix(lv_color_hex(c_.chain[3]), lv_color_hex(c_.panel), 110);
    tile.border_width = 1;
    tile.border_opa = LV_OPA_COVER;
    const lv_area_t t{x, y, x + kTile - 1, y + kTile - 1};
    lv_draw_rect(layer, &tile, &t);

    // The zero: a thick ring.
    const int cx = x + kTile / 2, cy = y + kTile / 2;
    constexpr int kR = 36, kRing = 7;
    lv_draw_rect_dsc_t ring;
    lv_draw_rect_dsc_init(&ring);
    ring.radius = LV_RADIUS_CIRCLE;
    ring.bg_opa = LV_OPA_TRANSP;
    ring.border_color = lv_color_hex(c_.accent);
    ring.border_width = kRing;
    ring.border_opa = LV_OPA_COVER;
    const lv_area_t r{cx - kR, cy - kR, cx + kR - 1, cy + kR - 1};
    lv_draw_rect(layer, &ring, &r);

    // Eight track bars inside the ring, rising from a common floor.
    constexpr int kBarW = 4, kGap = 2, kMaxH = 34;
    const int total = 8 * kBarW + 7 * kGap;
    const int x0 = cx - total / 2;
    const int floor = cy + kMaxH / 2;
    for (int i = 0; i < 8; ++i) {
        const int bh = std::max(3, static_cast<int>(std::lround(bars_[i] * kMaxH)));
        const int bx = x0 + i * (kBarW + kGap);
        fill(layer, {bx, floor - bh + 1, bx + kBarW - 1, floor}, scale(c_.chain, i / 7.0f), 2);
    }
}

void HomeView::draw(lv_layer_t* layer, const lv_area_t& box) const {
    fill(layer, box, lv_color_hex(c_.bg));
    draw_logo(layer, box.x1 + kTileX, box.y1 + kTileY);

    // Version under the tile.
    text(layer, line(box.x1 + kTileX, box.x1 + kTileX + kTile - 1, box.y1 + kTileY + kTile + 6, f_.small),
         version_.c_str(), f_.small, lv_color_hex(c_.fg), LV_TEXT_ALIGN_CENTER);

    // Name: "Zero" bright, "Tracker" in the accent; the tagline under it.
    const int tx = box.x1 + kMenuX + 2;
    const int ty = box.y1 + 12;
    const lv_area_t name = line(tx, box.x2, ty, f_.title);
    text(layer, name, "Zero", f_.title, lv_color_hex(c_.hi));
    lv_point_t zero_size;
    lv_text_get_size(&zero_size, "Zero", f_.title, 0, 0, LV_COORD_MAX, LV_TEXT_FLAG_NONE);
    text(layer, line(tx + zero_size.x, box.x2, ty, f_.title), "Tracker", f_.title,
         lv_color_hex(c_.accent));
    text(layer, line(tx, box.x2, ty + lv_font_get_line_height(f_.title) + 1, f_.small),
         tagline_.c_str(), f_.small, lv_color_hex(c_.dim));

    // Menu: icon, label, detail; the selected row on a pill.
    for (int i = 0; i < static_cast<int>(items_.size()); ++i) {
        const Item& it = items_[i];
        const int y = box.y1 + kMenuY + i * kRowH;
        const lv_area_t row{box.x1 + kMenuX, y, box.x2 - 6, y + kRowH - 4};
        const bool sel = i == selected_;
        if (sel) fill(layer, row, lv_color_hex(c_.cursor), 7);
        const uint32_t fg = !it.enabled ? c_.dim : sel ? c_.on_cursor : c_.fg;
        const uint32_t icon = !it.enabled ? c_.dim : sel ? c_.on_cursor : c_.accent;
        const int mid = (row.y1 + row.y2) / 2;
        if (it.icon) {
            const int ih = lv_font_get_line_height(f_.icon);
            text(layer, {row.x1 + 7, mid - ih / 2, row.x1 + 27, mid - ih / 2 + ih - 1}, it.icon,
                 f_.icon, lv_color_hex(icon), LV_TEXT_ALIGN_CENTER);
        }
        const int lh = lv_font_get_line_height(f_.label);
        text(layer, line(row.x1 + 32, row.x2, mid - lh / 2, f_.label), it.label.c_str(), f_.label,
             lv_color_hex(fg));
        const int sh = lv_font_get_line_height(f_.small);
        text(layer, line(row.x1 + 32, row.x2 - 7, mid - sh / 2 + 1, f_.small), it.detail.c_str(),
             f_.small, lv_color_hex(sel ? c_.on_cursor : it.enabled ? c_.fg : c_.dim),
             LV_TEXT_ALIGN_RIGHT);
    }
}

} // namespace screen
