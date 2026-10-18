/*
 * CHAIN | PHRASE: the chain on the left, the phrase of
 * its selected row on the right, split by a vertical line, each under its
 * title ("CHAIN 0A", "PHRASE 1F"). Rows alternate black and grey; notes and
 * chain numbers in bold, instruments and FX in colour. The two columns
 * scroll on their own, each following its cursor. TABLE is the same pane on
 * its own, full width (TSP, VOL, three FX). One LVGL object, one draw
 * callback, like the TIMELINE. It uses the grid's font, so it follows the
 * APP font size; the columns have fixed positions, sized for the
 * largest font.
 */
#pragma once

#include "lvgl.h"
#include "zt_viewer.h"

#include <array>
#include <cstdint>

namespace screen {

class ChainPhraseView {
public:
    struct Colors {
        uint32_t bg, panel, fg, dim, hi, accent, cursor, on_cursor, selection, playing, on_playing;
        uint32_t instrument, fx;
        std::array<uint32_t, 5> chain;  // phrase numbers, as the TIMELINE's chains
    };

    // `small_bold`: titles wider than their column at large font sizes.
    void create(lv_obj_t* parent, int x, int y, int w, int h, const lv_font_t* font,
                const lv_font_t* bold_font, const lv_font_t* small_bold, const Colors& colors);
    void set_visible(bool visible);
    // Edit mode: the cursor takes `color` (the theme's alert colour).
    void set_editing(bool editing, uint32_t color);
    // Redraws only when the snapshot changed.
    void set(const ZtChainPhrase& v);

private:
    static void draw_cb(lv_event_t* e);
    void draw(lv_layer_t* layer, const lv_area_t& box) const;
    void draw_pane(lv_layer_t* layer, int x0, int x1, int y0, const char* title, bool focused,
                   int top, int rows, int count, int cursor_row, int cursor_field, int play_row,
                   int selected_row, const ZtSplitCell* cells, int fields, const int* xs,
                   const int* ws, int shown) const;  // `fields` per row, the first `shown` drawn
    int visible_rows() const;
    // Title band: one bold line and its underline.
    int header_h() const { return lv_font_get_line_height(bold_) + 3; }

    lv_obj_t* obj_{nullptr};
    const lv_font_t* font_{nullptr};
    const lv_font_t* bold_{nullptr};
    const lv_font_t* small_bold_{nullptr};
    Colors c_{};
    int w_{0}, h_{0};
    int glyph_w_{7};
    int row_h_{13};
    ZtChainPhrase v_{};
    bool valid_{false};
    bool editing_{false};
    uint32_t edit_color_{0};
    // First row shown in each column; each keeps its cursor in view.
    int chain_top_{0};
    int phrase_top_{0};
};

} // namespace screen
