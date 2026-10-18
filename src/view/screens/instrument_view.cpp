/*
 * InstrumentView. See instrument_view.h.
 */
#include "instrument_view.h"

#include <algorithm>
#include <cctype>
#include <cmath>
#include <cstdlib>
#include <cstring>
#include <string>

namespace screen {
namespace {

// Bar size (px): at most this long, and no bar where less room is left.
constexpr int kBarMaxW = 56;
constexpr int kBarMinW = 14;
constexpr int kBarH = 6;

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
    lv_draw_label(layer, &dsc, &a);
}

lv_color_t scale_color(const std::array<uint32_t, 5>& stops, double t) {
    const double p = std::clamp(t, 0.0, 1.0) * 4;
    const int i = std::min(static_cast<int>(p), 3);
    const auto mix = static_cast<uint8_t>(std::lround((p - i) * 255.0));
    // lv_color_mix(a, b, r): r of a, 255 - r of b.
    return lv_color_mix(lv_color_hex(stops[i + 1]), lv_color_hex(stops[i]), mix);
}

} // namespace

void InstrumentView::create(lv_obj_t* parent, int x, int y, int w, int h, const lv_font_t* font,
                            const lv_font_t* bold_font, int cell_w, int cell_h, const Colors& colors) {
    obj_ = lv_obj_create(parent);
    lv_obj_remove_style_all(obj_);
    lv_obj_set_pos(obj_, x, y);
    lv_obj_set_size(obj_, w, h);
    lv_obj_remove_flag(obj_, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(obj_, draw_cb, LV_EVENT_DRAW_MAIN, this);
    font_ = font;
    bold_ = bold_font ? bold_font : font;
    c_ = colors;
    w_ = w;
    h_ = h;
    cell_w_ = std::max(1, cell_w);
    cell_h_ = std::max(1, cell_h);
    valid_ = false;
}

void InstrumentView::set_editing(bool editing, uint32_t color) {
    if (editing == editing_ && color == edit_color_) return;
    editing_ = editing;
    edit_color_ = color;
    if (obj_) lv_obj_invalidate(obj_);
}

void InstrumentView::set_visible(bool visible) {
    if (!obj_) return;
    if (visible) lv_obj_remove_flag(obj_, LV_OBJ_FLAG_HIDDEN);
    else lv_obj_add_flag(obj_, LV_OBJ_FLAG_HIDDEN);
}

void InstrumentView::set(const ZtInstrumentPage& page) {
    if (valid_ && std::memcmp(&page, &page_, sizeof page) == 0) return;
    page_ = page;
    valid_ = true;
    // The right-hand column: on each line the first label after a value with
    // two spaces before it; the most common one across the page wins.
    int votes[64] = {};
    for (int l = 0; l < page_.line_count; ++l) {
        const std::string line(page_.lines[l], strnlen(page_.lines[l], sizeof page_.lines[l]));
        std::string mask(line.size(), ' ');
        for (int s = 0; s < page_.span_count; ++s) {
            const ZtPageSpan& sp = page_.spans[s];
            if (sp.line != l) continue;
            for (int c = sp.column; c < sp.column + sp.width && c < static_cast<int>(mask.size()); ++c) mask[c] = 'v';
        }
        bool seen_value = false;
        for (size_t c = 2; c < line.size() && c < 64; ++c) {
            if (mask[c] == 'v') seen_value = true;
            if (seen_value && line[c] != ' ' && mask[c] != 'v' && line[c - 1] == ' ' && line[c - 2] == ' ') {
                ++votes[c];
                break;
            }
        }
    }
    right_col_ = 1 << 20;
    int best = 0;
    for (int c = 0; c < 64; ++c) {
        if (votes[c] > best) {
            best = votes[c];
            right_col_ = c;
        }
    }
    if (obj_) lv_obj_invalidate(obj_);
}

void InstrumentView::draw_cb(lv_event_t* e) {
    auto* self = static_cast<InstrumentView*>(lv_event_get_user_data(e));
    lv_area_t box;
    lv_obj_get_coords(self->obj_, &box);
    self->draw(lv_event_get_layer(e), box);
}

void InstrumentView::draw(lv_layer_t* layer, const lv_area_t& box) const {
    fill(layer, box, lv_color_hex(c_.bg));
    if (!valid_) return;
    const int fh = lv_font_get_line_height(font_);
    const int cols = w_ / cell_w_;
    for (int l = 0; l < page_.line_count; ++l) {
        const std::string line(page_.lines[l], strnlen(page_.lines[l], sizeof page_.lines[l]));
        const int y = box.y1 + l * cell_h_;
        if (l > 0 && l % 2 == 0) fill(layer, {box.x1, y, box.x2, y + cell_h_ - 1}, lv_color_hex(c_.panel));
        const int ty = y + (cell_h_ - fh) / 2;

        // Which columns hold a value.
        std::string mask(line.size(), ' ');
        for (int s = 0; s < page_.span_count; ++s) {
            const ZtPageSpan& sp = page_.spans[s];
            if (sp.line != l) continue;
            for (int c = sp.column; c < sp.column + sp.width && c < static_cast<int>(mask.size()); ++c) mask[c] = 'v';
        }

        // The right-hand column (DRY, AMP, LIM...) moves right: the left
        // labels are longer. Its column is found once for
        // the page; a line moves from there when a label starts exactly there.
        int right_col = 1 << 20;
        // A label glued to the text before it (MOD2 01ADSR ENVMOD4) moves too.
        if (right_col_ < static_cast<int>(line.size()) && line[right_col_] != ' ' &&
            mask[right_col_] != 'v' &&
            (line[right_col_ - 1] == ' ' || line.compare(right_col_, 3, "MOD") == 0)) {
            right_col = right_col_;
        }
        const int shift = 2 * cell_w_;
        auto x_of = [&](int col) { return box.x1 + col * cell_w_ + (col >= right_col ? shift : 0); };
        auto at = [&](int col, int chars) {
            return lv_area_t{x_of(col), ty, x_of(col) + chars * cell_w_ + 2, ty + fh - 1};
        };

        // Labels: the words outside the values, in bold white (the title
        // line, INST. xx, in the theme's colour).
        for (size_t c = 0; c < line.size();) {
            if (line[c] == ' ' || mask[c] == 'v') {
                ++c;
                continue;
            }
            size_t e = c;
            while (e < line.size() && line[e] != ' ' && mask[e] != 'v' &&
                   !(e > c && static_cast<int>(e) == right_col)) {
                ++e;
            }
            const std::string word = line.substr(c, e - c);
            lv_area_t a = at(static_cast<int>(c), static_cast<int>(e - c));
            // Text glued to a value (02HIGHPAS, 00CLIP, 14(020)) stands a
            // little apart.
            const bool is_word = word.size() >= 2 &&
                                 (std::isalpha(static_cast<unsigned char>(word[0])) || word[0] == '(');
            if (c > 0 && mask[c - 1] == 'v' && is_word) {
                a.x1 += 5;
                a.x2 += 5;
            }
            text(layer, a, word.c_str(), bold_, lv_color_hex(l == 0 ? c_.accent : c_.hi));
            c = e;
        }

        // Values: text in the accent colour, numbers in the text colour with
        // a bar in the free space after them.
        for (int s = 0; s < page_.span_count; ++s) {
            const ZtPageSpan& sp = page_.spans[s];
            if (sp.line != l || sp.column >= line.size()) continue;
            const std::string value = line.substr(sp.column, sp.width);
            const bool cursor = page_.cursor_line == l && page_.cursor_column == sp.column;
            const lv_area_t a = at(sp.column, sp.width);
            if (cursor) fill(layer, {a.x1 - 2, y + 1, a.x2, y + cell_h_ - 2}, lv_color_hex(editing_ ? edit_color_ : c_.cursor), 3);
            const uint32_t color = cursor ? c_.on_cursor : sp.kind == 0 ? c_.accent : c_.fg;
            text(layer, a, value.c_str(), sp.kind == 0 ? font_ : bold_, lv_color_hex(color));
            if (sp.kind == 0) continue;
            // A bar only for a value that stands alone after its label: not in
            // a run of values like RATIO 05.22 05.79 or LEV/FB AF/2F.
            int prev = static_cast<int>(sp.column) - 1;
            if (prev < 0 || line[prev] != ' ') continue;
            while (prev >= 0 && line[prev] == ' ') --prev;
            if (prev < 0 || mask[prev] == 'v' || !std::isalnum(static_cast<unsigned char>(line[prev]))) continue;

            // Room up to the next text on the line (the right-hand column's
            // label) or the edge of the screen.
            int next = sp.column + sp.width;
            while (next < static_cast<int>(line.size()) && line[next] == ' ') ++next;
            const int bx = x_of(sp.column) + sp.width * cell_w_ + 6;
            const int end_x = next < static_cast<int>(line.size()) ? x_of(next) : box.x1 + cols * cell_w_;
            const int room = end_x - 10 - bx;
            const int bw = std::min(kBarMaxW, room);
            if (bw < kBarMinW) continue;
            const int v = static_cast<int>(std::strtol(value.c_str(), nullptr, 16));
            const int by = y + (cell_h_ - kBarH) / 2;
            fill(layer, {bx, by, bx + bw - 1, by + kBarH - 1}, lv_color_hex(c_.bar_track), 2);
            const lv_color_t bar = scale_color(c_.chain, v / 255.0);
            if (sp.kind == 1) {
                const int len = (bw * v + 127) / 255;
                if (len > 0) fill(layer, {bx, by, bx + len - 1, by + kBarH - 1}, bar, 2);
            }
            else {
                // Centred on 80: from the middle to the value.
                const int mid = bx + bw / 2;
                const int len = (bw / 2) * (v - 0x80) / 0x80;
                const int x1 = std::min(mid, mid + len), x2 = std::max(mid, mid + len);
                fill(layer, {x1, by, std::max(x1, x2), by + kBarH - 1}, bar, 1);
                fill(layer, {mid, by - 1, mid, by + kBarH}, lv_color_hex(c_.hi));
            }
        }
    }
}

} // namespace screen
