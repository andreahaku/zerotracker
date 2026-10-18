/*
 * AppView. See app_view.h.
 */
#include "app_view.h"

#include <algorithm>
#include <cstdio>

namespace screen {
namespace {

// Phosphor Fill, the set the home screen uses.
constexpr const char* kIconFont = "";      // text-aa
constexpr const char* kIconTheme = "";     // palette
constexpr const char* kIconSpectrum = "";  // equalizer
constexpr const char* kIconLanguage = "";  // translate
constexpr const char* kIconInfo = "";      // info

constexpr int kRowH = 25;  // as the home menu
constexpr int kTop = 4;

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

void line(lv_layer_t* layer, int x1, int y1, int x2, int y2, lv_color_t color, int width) {
    lv_draw_line_dsc_t dsc;
    lv_draw_line_dsc_init(&dsc);
    dsc.color = color;
    dsc.width = width;
    dsc.p1.x = x1;
    dsc.p1.y = y1;
    dsc.p2.x = x2;
    dsc.p2.y = y2;
    lv_draw_line(layer, &dsc);
}

int wrapped_height(const std::string& s, const lv_font_t* font, int width) {
    lv_point_t p;
    lv_text_get_size(&p, s.c_str(), font, 0, 2, width, LV_TEXT_FLAG_NONE);
    return p.y;
}

// A small flag for each UI language, drawn (no image assets).
void flag(lv_layer_t* layer, int x, int y, int w, int h, zt::i18n::Lang lang) {
    using zt::i18n::Lang;
    auto stripes = [&](bool vertical, std::initializer_list<uint32_t> colors, std::initializer_list<int> parts) {
        int total = 0;
        for (int p : parts) total += p;
        int at = 0;
        auto c = colors.begin();
        for (int p : parts) {
            if (vertical) {
                const int x1 = x + w * at / total, x2 = x + w * (at + p) / total - 1;
                fill(layer, {x1, y, x2, y + h - 1}, lv_color_hex(*c));
            }
            else {
                const int y1 = y + h * at / total, y2 = y + h * (at + p) / total - 1;
                fill(layer, {x, y1, x + w - 1, y2}, lv_color_hex(*c));
            }
            at += p;
            ++c;
        }
    };
    switch (lang) {
        case Lang::It: stripes(true, {0x009246, 0xFFFFFF, 0xCE2B37}, {1, 1, 1}); break;
        case Lang::Fr: stripes(true, {0x0055A4, 0xFFFFFF, 0xEF4135}, {1, 1, 1}); break;
        case Lang::De: stripes(false, {0x000000, 0xDD0000, 0xFFCE00}, {1, 1, 1}); break;
        case Lang::Es: stripes(false, {0xAA151B, 0xF1BF00, 0xAA151B}, {1, 2, 1}); break;
        case Lang::Ja: {
            fill(layer, {x, y, x + w - 1, y + h - 1}, lv_color_hex(0xFFFFFF));
            const int r = h * 3 / 10, cx = x + w / 2, cy = y + h / 2;
            fill(layer, {cx - r, cy - r, cx + r, cy + r}, lv_color_hex(0xBC002D), LV_RADIUS_CIRCLE);
            break;
        }
        case Lang::Zh: {
            fill(layer, {x, y, x + w - 1, y + h - 1}, lv_color_hex(0xDE2910));
            const int r = h / 5;
            fill(layer, {x + 3, y + 2, x + 3 + 2 * r, y + 2 + 2 * r}, lv_color_hex(0xFFDE00), LV_RADIUS_CIRCLE);
            for (int k = 0; k < 4; ++k) {
                const int sx = x + 3 + 2 * r + 3 + (k % 2) * 2, sy = y + 1 + k * 2;
                fill(layer, {sx, sy, sx + 1, sy + 1}, lv_color_hex(0xFFDE00));
            }
            break;
        }
        case Lang::En:
        default: {
            // The Union Flag, simplified: blue, the diagonals, the cross.
            fill(layer, {x, y, x + w - 1, y + h - 1}, lv_color_hex(0x012169));
            line(layer, x, y, x + w - 1, y + h - 1, lv_color_hex(0xFFFFFF), 3);
            line(layer, x, y + h - 1, x + w - 1, y, lv_color_hex(0xFFFFFF), 3);
            line(layer, x, y, x + w - 1, y + h - 1, lv_color_hex(0xC8102E), 1);
            line(layer, x, y + h - 1, x + w - 1, y, lv_color_hex(0xC8102E), 1);
            fill(layer, {x + w / 2 - 2, y, x + w / 2 + 2, y + h - 1}, lv_color_hex(0xFFFFFF));
            fill(layer, {x, y + h / 2 - 2, x + w - 1, y + h / 2 + 2}, lv_color_hex(0xFFFFFF));
            fill(layer, {x + w / 2 - 1, y, x + w / 2 + 1, y + h - 1}, lv_color_hex(0xC8102E));
            fill(layer, {x, y + h / 2 - 1, x + w - 1, y + h / 2 + 1}, lv_color_hex(0xC8102E));
            break;
        }
    }
}

} // namespace

void AppView::create(lv_obj_t* parent, int x, int y, int w, int h, const Fonts& fonts,
                     const Colors& colors) {
    obj_ = lv_obj_create(parent);
    lv_obj_remove_style_all(obj_);
    lv_obj_set_pos(obj_, x, y);
    lv_obj_set_size(obj_, w, h);
    lv_obj_remove_flag(obj_, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(obj_, draw_cb, LV_EVENT_DRAW_MAIN, this);
    f_ = fonts;
    c_ = colors;
    w_ = w;
    h_ = h;
}

void AppView::set_visible(bool visible) {
    if (!obj_) return;
    if (visible) lv_obj_remove_flag(obj_, LV_OBJ_FLAG_HIDDEN);
    else lv_obj_add_flag(obj_, LV_OBJ_FLAG_HIDDEN);
}

void AppView::set(State state) {
    s_ = std::move(state);
    if (obj_) lv_obj_invalidate(obj_);
}

int AppView::content_height() const {
    const int sh = lv_font_get_line_height(f_.small);
    int y = kTop + 4 * kRowH + 6 + lv_font_get_line_height(f_.label) + 4;
    for (const auto& p : s_.help) y += wrapped_height(p, f_.small, w_ - 16) + sh / 2;
    return y + 4;
}

void AppView::draw_cb(lv_event_t* e) {
    auto* self = static_cast<AppView*>(lv_event_get_user_data(e));
    lv_area_t box;
    lv_obj_get_coords(self->obj_, &box);
    self->draw(lv_event_get_layer(e), box);
}

void AppView::draw(lv_layer_t* layer, const lv_area_t& box) const {
    fill(layer, box, lv_color_hex(c_.bg));
    const int top = box.y1 + kTop - s_.scroll;
    const int ih = lv_font_get_line_height(f_.icon);
    const int lh = lv_font_get_line_height(f_.label);
    const int sh = lv_font_get_line_height(f_.small);
    const char* icons[4] = {kIconFont, kIconTheme, kIconSpectrum, kIconLanguage};

    for (int i = 0; i < 4; ++i) {
        const lv_area_t row{box.x1 + 4, top + i * kRowH, box.x2 - 4, top + i * kRowH + kRowH - 4};
        if (row.y2 < box.y1 || row.y1 > box.y2) continue;
        const bool sel = i == s_.selected;
        if (sel) fill(layer, row, lv_color_hex(c_.cursor), 7);
        const int mid = (row.y1 + row.y2) / 2;
        const uint32_t fg = sel ? c_.on_cursor : c_.fg;
        text(layer, {row.x1 + 4, mid - ih / 2, row.x1 + 24, mid - ih / 2 + ih - 1}, icons[i], f_.icon,
             lv_color_hex(sel ? c_.on_cursor : c_.accent), LV_TEXT_ALIGN_CENTER);
        text(layer, {row.x1 + 30, mid - lh / 2, row.x1 + 170, mid + lh / 2}, s_.labels[i].c_str(), f_.label,
             lv_color_hex(fg));
        const int right = row.x2 - 8;
        switch (i) {
            case 0: {  // font size, with LEFT/RIGHT marks
                char v[16];
                std::snprintf(v, sizeof v, "< %d >", s_.font_size);
                text(layer, {right - 80, mid - lh / 2, right, mid + lh / 2}, v, f_.label, lv_color_hex(fg),
                     LV_TEXT_ALIGN_RIGHT);
                break;
            }
            case 1: {  // the theme: its name and colours
                const int sw = 9;
                for (int k = 0; k < 5; ++k) {
                    const int x = right - (5 - k) * (sw + 2);
                    fill(layer, {x, mid - sw / 2, x + sw - 1, mid + sw / 2}, lv_color_hex(s_.swatches[k]), 2);
                }
                text(layer, {right - 5 * (sw + 2) - 110, mid - sh / 2, right - 5 * (sw + 2) - 6, mid + sh / 2},
                     s_.theme.c_str(), f_.small, lv_color_hex(fg), LV_TEXT_ALIGN_RIGHT);
                break;
            }
            case 2: {  // the spectrum: a switch
                const int sw = 28, sh2 = 14;
                const lv_area_t track{right - sw, mid - sh2 / 2, right, mid + sh2 / 2};
                fill(layer, track, lv_color_hex(s_.spectrum ? (sel ? c_.on_cursor : c_.accent) : c_.dim),
                     LV_RADIUS_CIRCLE);
                const int kx = s_.spectrum ? track.x2 - sh2 + 2 : track.x1 + 2;
                fill(layer, {kx, track.y1 + 2, kx + sh2 - 5, track.y2 - 2},
                     lv_color_hex(s_.spectrum ? c_.bg : c_.fg), LV_RADIUS_CIRCLE);
                break;
            }
            case 3: {  // the language: its flag and its own name
                const int fw = 20, fh = 13;
                flag(layer, right - fw, mid - fh / 2, fw, fh, s_.lang);
                text(layer, {right - fw - 130, mid - lh / 2, right - fw - 6, mid + lh / 2},
                     zt::i18n::native_name(s_.lang), f_.label, lv_color_hex(fg), LV_TEXT_ALIGN_RIGHT);
                break;
            }
        }
    }

    // The instructions: a title with the version, then the paragraphs.
    int y = top + 4 * kRowH + 6;
    text(layer, {box.x1 + 8, y, box.x1 + 28, y + ih - 1}, kIconInfo, f_.icon, lv_color_hex(c_.accent),
         LV_TEXT_ALIGN_CENTER);
    text(layer, {box.x1 + 34, y + (ih - lh) / 2, box.x2 - 70, y + (ih + lh) / 2}, s_.help_title.c_str(),
         f_.label, lv_color_hex(c_.hi));
    text(layer, {box.x2 - 70, y + (ih - sh) / 2, box.x2 - 8, y + (ih + sh) / 2}, s_.version.c_str(), f_.small,
         lv_color_hex(c_.fg), LV_TEXT_ALIGN_RIGHT);
    y += lh + 6;
    const int width = w_ - 16;
    for (const auto& p : s_.help) {
        const int ph = wrapped_height(p, f_.small, width);
        if (y + ph >= box.y1 && y <= box.y2) {
            lv_draw_label_dsc_t dsc;
            lv_draw_label_dsc_init(&dsc);
            dsc.text = p.c_str();
            dsc.text_local = 1;
            dsc.font = f_.small;
            dsc.color = lv_color_hex(c_.fg);
            dsc.line_space = 2;
            const lv_area_t a{box.x1 + 8, y, box.x1 + 8 + width - 1, y + ph};
            lv_draw_label(layer, &dsc, &a);
        }
        y += ph + sh / 2;
    }
}

} // namespace screen
