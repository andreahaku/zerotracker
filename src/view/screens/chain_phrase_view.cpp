/*
 * ChainPhraseView. See chain_phrase_view.h.
 */
#include "chain_phrase_view.h"

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>

namespace screen {
namespace {

constexpr int kChainFields = 2;
constexpr int kPhraseFields = 9;
// Fixed columns, in px from the column's left edge,
// sized for the largest font (17 px: 3 characters in 31 px); smaller fonts
// keep the same positions. Row number first, then the fields.
constexpr int kIndexX = 3, kIndexW = 10;
constexpr int kChainX[kChainFields] = {15, 38};
constexpr int kChainW[kChainFields] = {21, 21};
constexpr int kChainPaneW = 60;
constexpr int kPhraseX[kPhraseFields] = {15, 48, 71, 95, 126, 150, 181, 205, 236};
constexpr int kPhraseW[kPhraseFields] = {31, 21, 21, 31, 21, 31, 21, 31, 21};
// TABLE, alone on the full width: TSP, VOL, then three FX with their values.
constexpr int kTableFields = 8;
constexpr int kTableX[kTableFields] = {18, 48, 84, 117, 154, 187, 224, 257};
constexpr int kTableW[kTableFields] = {21, 21, 31, 21, 31, 21, 31, 21};

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

// Phrase numbers in the chain take the TIMELINE's chain scale.
lv_color_t scale_color(const std::array<uint32_t, 5>& stops, int value) {
    const double t = std::min(value, 0x8F) / double{0x8F} * 4;
    const int i = std::min(static_cast<int>(t), 3);
    const auto mix = static_cast<uint8_t>(std::lround((t - i) * 255.0));
    // lv_color_mix(a, b, r): r of a, 255 - r of b.
    return lv_color_mix(lv_color_hex(stops[i + 1]), lv_color_hex(stops[i]), mix);
}

// First row shown so that `row` stays in a window of `rows`.
int follow(int top, int row, int rows) {
    if (row < top) return row;
    if (row >= top + rows) return row - rows + 1;
    return std::clamp(top, 0, std::max(0, 16 - rows));
}

} // namespace

void ChainPhraseView::create(lv_obj_t* parent, int x, int y, int w, int h, const lv_font_t* font,
                             const lv_font_t* bold_font, const lv_font_t* small_bold,
                             const Colors& colors) {
    obj_ = lv_obj_create(parent);
    lv_obj_remove_style_all(obj_);
    lv_obj_set_pos(obj_, x, y);
    lv_obj_set_size(obj_, w, h);
    lv_obj_remove_flag(obj_, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(obj_, draw_cb, LV_EVENT_DRAW_MAIN, this);
    font_ = font;
    bold_ = bold_font ? bold_font : font;
    small_bold_ = small_bold ? small_bold : bold_;
    c_ = colors;
    w_ = w;
    h_ = h;
    glyph_w_ = std::max<int>(1, lv_font_get_glyph_width(font_, '0', 0));
    // Rows as tight as the digits allow: the font's line height has room for
    // descenders that hex digits and note names do not use.
    row_h_ = std::max(10, static_cast<int>(lv_font_get_line_height(font_)) - 3);
    valid_ = false;
}

void ChainPhraseView::set_editing(bool editing, uint32_t color) {
    if (editing == editing_ && color == edit_color_) return;
    editing_ = editing;
    edit_color_ = color;
    if (obj_) lv_obj_invalidate(obj_);
}

void ChainPhraseView::set_visible(bool visible) {
    if (!obj_) return;
    if (visible) lv_obj_remove_flag(obj_, LV_OBJ_FLAG_HIDDEN);
    else lv_obj_add_flag(obj_, LV_OBJ_FLAG_HIDDEN);
}

int ChainPhraseView::visible_rows() const {
    return std::clamp((h_ - header_h()) / std::max(1, row_h_), 1, 16);
}

void ChainPhraseView::set(const ZtChainPhrase& v) {
    if (valid_ && std::memcmp(&v, &v_, sizeof v) == 0) return;
    const int rows = visible_rows();
    // The chain keeps its selected row in view. The phrase follows its cursor
    // when it has the focus, else it starts at its top when it changes.
    chain_top_ = follow(chain_top_, v.chain_row, rows);
    if (v.focus == 1) {
        phrase_top_ = follow(phrase_top_, v.phrase_row, rows);
    }
    else if (v.phrase_play >= 0) {
        phrase_top_ = follow(phrase_top_, v.phrase_play, rows);  // follows the playhead
    }
    else if (!valid_ || v.phrase != v_.phrase || v_.focus == 1) {
        phrase_top_ = 0;
    }
    v_ = v;
    valid_ = true;
    if (obj_) lv_obj_invalidate(obj_);
}

void ChainPhraseView::draw_cb(lv_event_t* e) {
    auto* self = static_cast<ChainPhraseView*>(lv_event_get_user_data(e));
    lv_area_t box;
    lv_obj_get_coords(self->obj_, &box);
    self->draw(lv_event_get_layer(e), box);
}

void ChainPhraseView::draw_pane(lv_layer_t* layer, int x0, int x1, int y0, const char* title,
                                bool focused, int top, int rows, int count, int cursor_row,
                                int cursor_field, int play_row, int selected_row,
                                const ZtSplitCell* cells, int fields, const int* xs,
                                const int* ws, int shown) const {
    // Title, bright and underlined on the column with the cursor.
    // A title wider than its column is set in the small bold font.
    lv_point_t size;
    lv_text_get_size(&size, title, bold_, 0, 0, LV_COORD_MAX, LV_TEXT_FLAG_NONE);
    const lv_font_t* tf = size.x + 6 <= x1 - x0 ? bold_ : small_bold_;
    const int th = lv_font_get_line_height(tf);
    text(layer, {x0 + 4, y0 + (header_h() - th) / 2, x1, y0 + header_h()}, title, tf,
         lv_color_hex(focused ? c_.accent : c_.fg));
    if (focused) fill(layer, {x0 + 2, y0 + header_h() - 2, x1 - 2, y0 + header_h() - 1}, lv_color_hex(c_.accent));

    const int fh = lv_font_get_line_height(font_);
    for (int i = 0; i < std::min(rows, count - top); ++i) {
        const int row = top + i;
        const int y = y0 + header_h() + i * row_h_;
        const lv_area_t line{x0, y, x1, y + row_h_ - 1};
        // Rows alternate black and grey; the chain row whose phrase is shown
        // on the right is highlighted while the cursor is in the phrase.
        if (row == selected_row) fill(layer, line, lv_color_hex(c_.selection));
        else if (row % 2 == 1) fill(layer, line, lv_color_hex(c_.panel));
        const int ty = y + (row_h_ - fh) / 2;

        // Row number; on a pill in the playing colour when it plays.
        char idx[2];
        std::snprintf(idx, sizeof idx, "%X", row);
        const lv_area_t ia{x0 + kIndexX, ty, x0 + kIndexX + kIndexW - 1, ty + fh - 1};
        if (row == play_row) {
            fill(layer, {ia.x1 - 1, y + 1, ia.x2 + 1, y + row_h_ - 2}, lv_color_hex(c_.playing), 3);
        }
        text(layer, ia, idx, font_, lv_color_hex(row == play_row ? c_.on_playing : c_.dim),
             LV_TEXT_ALIGN_CENTER);

        for (int f = 0; f < std::min(fields, shown); ++f) {
            const lv_area_t a{x0 + xs[f], ty, x0 + xs[f] + ws[f] - 1, ty + fh - 1};
            const ZtSplitCell& cell = cells[row * fields + f];
            const bool on_cursor = focused && row == cursor_row && f == cursor_field;
            if (on_cursor) {
                // The cursor wraps the text, not the whole fixed column.
                lv_point_t size;
                lv_text_get_size(&size, cell.text[0] ? cell.text : "--", bold_, 0, 0, LV_COORD_MAX,
                                 LV_TEXT_FLAG_NONE);
                fill(layer, {a.x1 - 2, y + 1, a.x1 + size.x + 1, y + row_h_ - 2}, lv_color_hex(editing_ ? edit_color_ : c_.cursor), 3);
            }
            lv_color_t color = lv_color_hex(c_.dim);
            const lv_font_t* font = font_;
            switch (cell.role) {
            case ZT_CELL_PHRASE:
                color = scale_color(c_.chain, static_cast<int>(std::strtol(cell.text, nullptr, 16)));
                font = bold_;
                break;
            case ZT_CELL_NOTE: color = lv_color_hex(c_.hi); font = bold_; break;
            case ZT_CELL_INSTRUMENT: color = lv_color_hex(c_.instrument); break;
            case ZT_CELL_FX_COMMAND: color = lv_color_hex(c_.fx); font = bold_; break;
            case ZT_CELL_TRANSPOSE:
            case ZT_CELL_VELOCITY:
            case ZT_CELL_FX_VALUE: color = lv_color_hex(c_.fg); break;
            default: break;
            }
            if (on_cursor) color = lv_color_hex(c_.on_cursor);
            text(layer, a, cell.text, font, color);
        }
    }

    // Scroll bar when the 16 rows do not fit.
    if (rows < count) {
        const int y1 = y0 + header_h(), y2 = y0 + header_h() + rows * row_h_ - 1;
        const int track = y2 - y1 + 1;
        const int thumb = std::max(6, track * rows / count);
        const int ty = y1 + (track - thumb) * top / std::max(1, count - rows);
        fill(layer, {x1 - 3, ty, x1 - 2, ty + thumb - 1}, lv_color_hex(c_.dim), 1);
    }
}

void ChainPhraseView::draw(lv_layer_t* layer, const lv_area_t& box) const {
    fill(layer, box, lv_color_hex(c_.bg));
    if (!valid_) return;
    const int rows = visible_rows();
    char title[16];
    if (v_.table) {
        std::snprintf(title, sizeof title, "TABLE %02X", v_.phrase);
        draw_pane(layer, box.x1, box.x2, box.y1, title, true, phrase_top_, rows, 16, v_.phrase_row,
                  v_.phrase_field, -1, -1, &v_.phrase_cells[0][0], kPhraseFields, kTableX, kTableW,
                  kTableFields);
        // The FX column names in the title band, past the title.
        const int sh = lv_font_get_line_height(small_bold_);
        for (int k = 0; k < 3; ++k) {
            char name[4];
            std::snprintf(name, sizeof name, "FX%d", k + 1);
            const int x = box.x1 + kTableX[2 + 2 * k];
            text(layer, {x, box.y1 + (header_h() - sh) / 2, x + 40, box.y1 + header_h()}, name, small_bold_,
                 lv_color_hex(c_.dim));
        }
        return;
    }
    const int split_x = box.x1 + kChainPaneW;
    std::snprintf(title, sizeof title, "CHAIN %02X", v_.chain);
    draw_pane(layer, box.x1, split_x - 1, box.y1, title, v_.focus == 0, chain_top_, rows, 16,
              v_.chain_row, v_.chain_field, v_.chain_play, v_.focus == 1 ? v_.chain_row : -1,
              &v_.chain_cells[0][0], kChainFields, kChainX, kChainW, kChainFields);

    // The vertical line between the two.
    fill(layer, {split_x, box.y1 + 2, split_x, box.y2 - 2}, lv_color_hex(c_.dim));

    if (v_.phrase_valid) {
        std::snprintf(title, sizeof title, "PHRASE %02X", v_.phrase);
        draw_pane(layer, split_x + 1, box.x2, box.y1, title, v_.focus == 1, phrase_top_, rows, 16,
                  v_.phrase_row, v_.phrase_field, v_.phrase_play, -1, &v_.phrase_cells[0][0],
                  kPhraseFields, kPhraseX, kPhraseW, kPhraseFields);
    }
    else {
        const int th = lv_font_get_line_height(bold_);
        text(layer, {split_x + 5, box.y1 + (header_h() - th) / 2, box.x2, box.y1 + header_h()}, "PHRASE --",
             bold_, lv_color_hex(c_.dim));
        const int fh = lv_font_get_line_height(font_);
        const int my = box.y1 + header_h() + (rows * row_h_ - fh) / 2;
        text(layer, {split_x + 1, my, box.x2, my + fh - 1}, "no phrase on this row", font_,
             lv_color_hex(c_.dim), LV_TEXT_ALIGN_CENTER);
    }
}

} // namespace screen
