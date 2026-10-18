/*
 * SongView. See song_view.h.
 */
#include "song_view.h"

#include <algorithm>
#include <cstdio>
#include <cstring>

namespace screen {
namespace {

// The home screen's glyphs, and Phosphor Fill ones of the same set.
constexpr const char* kIconSong = "";       // music-notes (the browser's songs)
constexpr const char* kIconSave = "";       // floppy-disk
constexpr const char* kIconNew = "";        // plus-circle (home NEW)
constexpr const char* kIconLoad = "";       // folder-open (home LOAD)
constexpr const char* kIconTempo = "";      // metronome
constexpr const char* kIconTranspose = "";  // arrows-down-up
constexpr const char* kIconRender = "";     // waveform (the browser's samples)

constexpr int kRowH = 25;  // as the home menu

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

lv_area_t line(int x1, int x2, int y, const lv_font_t* font) {
    return {x1, y, x2, y + lv_font_get_line_height(font) - 1};
}

int width_of(const char* s, const lv_font_t* font) {
    lv_point_t p;
    lv_text_get_size(&p, s, font, 0, 0, LV_COORD_MAX, LV_TEXT_FLAG_NONE);
    return p.x;
}

} // namespace

void SongView::create(lv_obj_t* parent, int x, int y, int w, int h, const Fonts& fonts,
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

void SongView::set_visible(bool visible) {
    if (!obj_) return;
    if (visible) lv_obj_remove_flag(obj_, LV_OBJ_FLAG_HIDDEN);
    else lv_obj_add_flag(obj_, LV_OBJ_FLAG_HIDDEN);
}

void SongView::set_render_status(const std::string& status, bool failed) {
    if (status == render_status_ && failed == render_failed_) return;
    render_status_ = status;
    render_failed_ = failed;
    if (obj_) lv_obj_invalidate(obj_);
}

void SongView::set(const ZtSongPage& page, const Words& words, const std::string& name_override) {
    if (std::memcmp(&page, &page_, sizeof page) == 0 && name_override == name_ &&
        words.save == words_.save) {
        return;
    }
    page_ = page;
    words_ = words;
    name_ = name_override;
    if (obj_) lv_obj_invalidate(obj_);
}

void SongView::draw_cb(lv_event_t* e) {
    auto* self = static_cast<SongView*>(lv_event_get_user_data(e));
    lv_area_t box;
    lv_obj_get_coords(self->obj_, &box);
    self->draw(lv_event_get_layer(e), box);
}

void SongView::draw(lv_layer_t* layer, const lv_area_t& box) const {
    fill(layer, box, lv_color_hex(c_.bg));
    const int field = page_.field;
    const uint32_t pill = page_.editing ? c_.alert : c_.cursor;
    const int ih = lv_font_get_line_height(f_.icon);
    const int lh = lv_font_get_line_height(f_.label);
    const int sh = lv_font_get_line_height(f_.small);
    auto icon = [&](int x, int mid, const char* glyph, uint32_t color) {
        text(layer, {x, mid - ih / 2, x + 20, mid - ih / 2 + ih - 1}, glyph, f_.icon, lv_color_hex(color),
             LV_TEXT_ALIGN_CENTER);
    };

    // Left column: NAME, TEMPO, TRANSPOSE.
    const int lx1 = box.x1 + 4, lx2 = box.x1 + 168;
    // NAME: the song icon and the name; the field is the row.
    {
        const lv_area_t row{lx1, box.y1 + 4, lx2, box.y1 + 4 + kRowH - 4};
        const bool sel = field == 0;
        if (sel) fill(layer, row, lv_color_hex(pill), 7);
        const int mid = (row.y1 + row.y2) / 2;
        icon(row.x1 + 4, mid, kIconSong, sel ? c_.on_cursor : c_.accent);
        const std::string name = !name_.empty() ? name_ : page_.name[0] ? page_.name : "------------";
        text(layer, line(row.x1 + 28, row.x2 - 4, mid - lh / 2, f_.label), name.c_str(), f_.label,
             lv_color_hex(sel ? c_.on_cursor : c_.hi));
    }
    // TEMPO: large, with BPM beside it.
    {
        const int bh = lv_font_get_line_height(f_.big);
        const lv_area_t row{lx1, box.y1 + 4 + kRowH, lx2, box.y1 + 4 + kRowH + bh + 4};
        const bool sel = field == 1;
        if (sel) fill(layer, row, lv_color_hex(pill), 7);
        const int mid = (row.y1 + row.y2) / 2;
        icon(row.x1 + 4, mid, kIconTempo, sel ? c_.on_cursor : c_.accent);
        char tempo[16];
        std::snprintf(tempo, sizeof tempo, "%.2f", static_cast<double>(page_.tempo));
        text(layer, line(row.x1 + 28, row.x2, mid - bh / 2, f_.big), tempo, f_.big,
             lv_color_hex(sel ? c_.on_cursor : c_.hi));
        const int tw = width_of(tempo, f_.big);
        // BPM beside it when it fits (large APP font sizes leave no room).
        if (row.x1 + 32 + tw + width_of("BPM", f_.small) <= row.x2) {
            text(layer, line(row.x1 + 32 + tw, row.x2, mid + bh / 2 - sh - 2, f_.small), "BPM", f_.small,
                 lv_color_hex(sel ? c_.on_cursor : c_.fg));
        }
    }
    // TRANSPOSE: signed semitones.
    {
        const int top = box.y1 + 4 + kRowH + lv_font_get_line_height(f_.big) + 8;
        const lv_area_t row{lx1, top, lx2, top + kRowH - 4};
        const bool sel = field == 2;
        if (sel) fill(layer, row, lv_color_hex(pill), 7);
        const int mid = (row.y1 + row.y2) / 2;
        icon(row.x1 + 4, mid, kIconTranspose, sel ? c_.on_cursor : c_.accent);
        const int semis = static_cast<int8_t>(page_.transpose);
        char value[16];
        std::snprintf(value, sizeof value, "%+d", semis);
        text(layer, line(row.x1 + 28, row.x2, mid - lh / 2, f_.label), value, f_.label,
             lv_color_hex(sel ? c_.on_cursor : c_.hi));
        char hex[8];
        std::snprintf(hex, sizeof hex, "%02X", page_.transpose);
        text(layer, line(row.x1 + 28, row.x2 - 8, mid - sh / 2 + 1, f_.small), hex, f_.small,
             lv_color_hex(sel ? c_.on_cursor : c_.fg), LV_TEXT_ALIGN_RIGHT);
    }

    // Right column: SAVE, NEW, LOAD as the home menu.
    const int rx1 = box.x1 + 176, rx2 = box.x2 - 4;
    struct Action {
        const char* icon;
        const std::string* label;
        std::string detail;
    };
    const std::string file = page_.file;
    const Action actions[4] = {
        {kIconSave, &words_.save, page_.dirty ? words_.unsaved : file},
        {kIconNew, &words_.fresh, ""},
        {kIconLoad, &words_.load, ""},
        {kIconRender, &words_.render, render_status_},
    };
    for (int i = 0; i < 4; ++i) {
        const lv_area_t row{rx1, box.y1 + 4 + i * kRowH, rx2, box.y1 + 4 + i * kRowH + kRowH - 4};
        const bool sel = field == 3 + i;
        if (sel) fill(layer, row, lv_color_hex(c_.cursor), 7);
        const int mid = (row.y1 + row.y2) / 2;
        icon(row.x1 + 4, mid, actions[i].icon, sel ? c_.on_cursor : c_.accent);
        text(layer, line(row.x1 + 28, row.x2, mid - lh / 2, f_.label), actions[i].label->c_str(),
             f_.label, lv_color_hex(sel ? c_.on_cursor : c_.fg));
        // The detail on the right when it fits beside the label.
        const int room = row.x2 - 6 - (row.x1 + 28 + width_of(actions[i].label->c_str(), f_.label) + 8);
        if (!actions[i].detail.empty() && width_of(actions[i].detail.c_str(), f_.small) <= room) {
            const bool warn = (i == 0 && page_.dirty) || (i == 3 && render_failed_);
            text(layer, line(row.x1 + 28, row.x2 - 6, mid - sh / 2 + 1, f_.small),
                 actions[i].detail.c_str(), f_.small,
                 lv_color_hex(sel ? c_.on_cursor : warn ? c_.alert : c_.fg), LV_TEXT_ALIGN_RIGHT);
        }
    }

    // Bottom: what the song uses, as chips.
    const int by = box.y2 - sh - 4;
    int x = box.x1 + 6;
    const std::pair<const std::string*, int> stats[4] = {
        {&words_.rows, page_.rows}, {&words_.chains, page_.chains},
        {&words_.phrases, page_.phrases}, {&words_.instruments, page_.instruments}};
    for (const auto& [label, n] : stats) {
        char chip[32];
        std::snprintf(chip, sizeof chip, "%d %s", n, label->c_str());
        const int w = width_of(chip, f_.small) + 10;
        if (x + w > box.x2 - 4) break;
        fill(layer, {x, by - 2, x + w, by + sh}, lv_color_hex(c_.panel), 5);
        text(layer, line(x + 5, x + w, by - 1, f_.small), chip, f_.small, lv_color_hex(c_.fg));
        x += w + 6;
    }
}

} // namespace screen
