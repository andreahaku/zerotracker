/*
 * BrowserView. See browser_view.h.
 */
#include "browser_view.h"

#include <algorithm>

namespace screen {
namespace {

constexpr int kHeaderH = 24;
constexpr int kRowH = 20;

// Phosphor glyphs (fill style).
constexpr const char* kIconFolderOpen = "";
constexpr const char* kIconFolder = "";
constexpr const char* kIconUp = "";          // caret-left: the folder above
constexpr const char* kIconSong = "";        // music-notes
constexpr const char* kIconInstrument = "";  // piano-keys
constexpr const char* kIconSample = "";      // waveform

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

constexpr const char* kIconInstalled = "\uE184";  // check-circle
constexpr const char* kIconDownload = "\uE1AC";   // cloud-arrow-down

// `s` cut to fit `width` px in `font` on one line, with "..." when cut.
// Byte-wise is enough: file names on the card are ASCII.
std::string fitted(const std::string& s, const lv_font_t* font, int width) {
    auto w = [font](const std::string& t) {
        lv_point_t p;
        lv_text_get_size(&p, t.c_str(), font, 0, 0, LV_COORD_MAX, LV_TEXT_FLAG_NONE);
        return static_cast<int>(p.x);
    };
    if (w(s) <= width) return s;
    std::string cut = s;
    while (!cut.empty() && w(cut + "...") > width) cut.pop_back();
    return cut + "...";
}

// One text line of `font`, centred on y = mid.
lv_area_t centred(int x1, int x2, int mid, const lv_font_t* font) {
    const int h = lv_font_get_line_height(font);
    return {x1, mid - h / 2, x2, mid - h / 2 + h - 1};
}

} // namespace

void BrowserView::create(lv_obj_t* parent, int x, int y, int w, int h, const Fonts& fonts,
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
    valid_ = false;
}

void BrowserView::set_words(const char* items, const char* empty) {
    items_word_ = items;
    empty_text_ = empty;
    valid_ = false;
}

void BrowserView::set_visible(bool visible) {
    if (!obj_) return;
    if (visible) lv_obj_remove_flag(obj_, LV_OBJ_FLAG_HIDDEN);
    else lv_obj_add_flag(obj_, LV_OBJ_FLAG_HIDDEN);
}

int BrowserView::visible_rows() const {
    return std::max(1, (h_ - kHeaderH - note_h() - 2) / row_h());
}

int BrowserView::row_h() const {
    return two_line_ ? kRowH + lv_font_get_line_height(f_.small) : kRowH;
}

int BrowserView::note_h() const {
    return note_.empty() ? 0 : lv_font_get_line_height(f_.small) + 4;
}

void BrowserView::set_note(const std::string& note) {
    if (note == note_) return;
    note_ = note;
    if (obj_) lv_obj_invalidate(obj_);
}

void BrowserView::set(const std::string& path, const std::vector<Row>& rows, int selected, int top,
                      const std::string& message) {
    const bool same = valid_ && path == path_ && selected == selected_ && top == top_ &&
                      message == message_ && rows.size() == rows_.size() &&
                      std::equal(rows.begin(), rows.end(), rows_.begin(), [](const Row& a, const Row& b) {
                          return a.kind == b.kind && a.name == b.name && a.detail == b.detail &&
                                 a.enabled == b.enabled && a.mark == b.mark && a.sub == b.sub;
                      });
    if (same) return;
    path_ = path;
    rows_ = rows;
    selected_ = selected;
    top_ = top;
    message_ = message;
    valid_ = true;
    if (obj_) lv_obj_invalidate(obj_);
}

void BrowserView::draw_cb(lv_event_t* e) {
    auto* self = static_cast<BrowserView*>(lv_event_get_user_data(e));
    lv_area_t box;
    lv_obj_get_coords(self->obj_, &box);
    self->draw(lv_event_get_layer(e), box);
}

void BrowserView::draw(lv_layer_t* layer, const lv_area_t& box) const {
    fill(layer, box, lv_color_hex(c_.bg));
    if (!valid_) return;

    // Header: open folder, the path (its end when long), the count or a message.
    const int hmid = box.y1 + kHeaderH / 2;
    fill(layer, {box.x1, box.y1, box.x2, box.y1 + kHeaderH - 1}, lv_color_hex(c_.panel));
    text(layer, centred(box.x1 + 6, box.x1 + 24, hmid, f_.icon), kIconFolderOpen, f_.icon,
         lv_color_hex(c_.folder), LV_TEXT_ALIGN_CENTER);
    const std::string right = message_.empty()
        ? std::to_string(rows_.size() - (!rows_.empty() && rows_[0].kind == Kind::Parent)) + " " + items_word_
        : message_;
    lv_point_t right_size;
    lv_text_get_size(&right_size, right.c_str(), f_.small, 0, 0, LV_COORD_MAX, LV_TEXT_FLAG_NONE);
    const int path_x2 = box.x2 - right_size.x - 14;
    std::string shown = path_;
    const int glyph = std::max<int>(1, lv_font_get_glyph_width(f_.header, 'M', 0));
    const size_t fit = static_cast<size_t>(std::max(4, (path_x2 - box.x1 - 30) / glyph));
    if (shown.size() > fit) shown = "..." + shown.substr(shown.size() - (fit - 3));
    text(layer, centred(box.x1 + 30, path_x2, hmid, f_.header), shown.c_str(), f_.header,
         lv_color_hex(c_.hi));
    text(layer, centred(box.x1, box.x2 - 8, hmid + 1, f_.small), right.c_str(), f_.small,
         lv_color_hex(message_.empty() ? c_.dim : c_.accent), LV_TEXT_ALIGN_RIGHT);

    // Rows.
    if (!note_.empty()) {
        const int y = box.y1 + kHeaderH + 2;
        text(layer, {box.x1 + 8, y, box.x2 - 8, y + lv_font_get_line_height(f_.small) - 1},
             fitted(note_, f_.small, box.x2 - box.x1 - 16).c_str(), f_.small, lv_color_hex(c_.dim));
    }
    const int list_y = box.y1 + kHeaderH + note_h() + 2;
    const int n = static_cast<int>(rows_.size());
    const int shown_rows = visible_rows();
    if (n == 0) {
        text(layer, centred(box.x1, box.x2, list_y + 2 * kRowH, f_.row), empty_text_.c_str(), f_.row,
             lv_color_hex(c_.dim), LV_TEXT_ALIGN_CENTER);
    }
    for (int i = top_; i < std::min(n, top_ + shown_rows); ++i) {
        const Row& r = rows_[i];
        const int y = list_y + (i - top_) * row_h();
        const lv_area_t row{box.x1 + 3, y, box.x2 - 9, y + row_h() - 2};
        // The name line's centre: the top line of a two-line row.
        const int mid = two_line_ ? row.y1 + kRowH / 2 : (row.y1 + row.y2) / 2;
        // A row that cannot be opened here is grey, on a dark pill when selected.
        const bool sel = i == selected_ && r.enabled;
        if (i == selected_) fill(layer, row, lv_color_hex(r.enabled ? c_.cursor : c_.panel), 6);
        const char* icon = kIconFolder;
        uint32_t icon_color = c_.folder;
        switch (r.kind) {
        case Kind::Parent: icon = kIconUp; icon_color = c_.dim; break;
        case Kind::Folder: break;
        case Kind::Song: icon = kIconSong; icon_color = c_.song; break;
        case Kind::Instrument: icon = kIconInstrument; icon_color = c_.instrument; break;
        case Kind::Sample: icon = kIconSample; icon_color = c_.sample; break;
        }
        if (!r.enabled) icon_color = c_.panel_text;
        text(layer, centred(row.x1 + 4, row.x1 + 22, mid, f_.icon), icon, f_.icon,
             lv_color_hex(sel ? c_.on_cursor : icon_color), LV_TEXT_ALIGN_CENTER);
        const uint32_t name_color = sel ? c_.on_cursor : r.enabled ? c_.fg : c_.panel_text;
        lv_point_t detail_size{0, 0};
        const uint32_t detail_color = sel ? c_.on_cursor : r.mark == 1 ? c_.accent : r.enabled ? c_.dim : c_.panel_text;
        if (!r.detail.empty()) {
            lv_text_get_size(&detail_size, r.detail.c_str(), f_.small, 0, 0, LV_COORD_MAX,
                             LV_TEXT_FLAG_NONE);
            text(layer, centred(row.x1, row.x2 - 6, mid + 1, f_.small), r.detail.c_str(), f_.small,
                 lv_color_hex(detail_color), LV_TEXT_ALIGN_RIGHT);
        }
        if (r.mark != 0) {
            // The status icon just left of the detail.
            const int x2 = row.x2 - 6 - detail_size.x - 4;
            text(layer, centred(x2 - 18, x2, mid, f_.icon), r.mark == 1 ? kIconInstalled : kIconDownload,
                 f_.icon, lv_color_hex(detail_color), LV_TEXT_ALIGN_CENTER);
            detail_size.x += 22;
        }
        lv_area_t na = centred(row.x1 + 28, row.x2 - detail_size.x - 12, mid, f_.row);
        const std::string name =
            fitted(r.kind == Kind::Parent ? "back" : r.name, f_.row, na.x2 - na.x1 + 1);
        lv_draw_label_dsc_t dsc;
        lv_draw_label_dsc_init(&dsc);
        dsc.text = name.c_str();
        dsc.text_local = 1;
        dsc.font = f_.row;
        dsc.color = lv_color_hex(name_color);
        dsc.flag = LV_TEXT_FLAG_NONE;
        lv_draw_label(layer, &dsc, &na);
        if (two_line_ && !r.sub.empty()) {
            const int sy = row.y1 + kRowH - 3;
            text(layer, {row.x1 + 28, sy, row.x2 - 6, sy + lv_font_get_line_height(f_.small) - 1},
                 fitted(r.sub, f_.small, row.x2 - row.x1 - 34).c_str(), f_.small,
                 lv_color_hex(sel ? c_.on_cursor : c_.dim));
        }
    }

    // Scroll bar when the list does not fit.
    if (n > shown_rows) {
        const int track_y1 = list_y, track_y2 = box.y2 - 2;
        const int track_h = track_y2 - track_y1 + 1;
        fill(layer, {box.x2 - 4, track_y1, box.x2 - 3, track_y2}, lv_color_hex(c_.panel), 1);
        const int thumb_h = std::max(8, track_h * shown_rows / n);
        const int thumb_y = track_y1 + (track_h - thumb_h) * top_ / std::max(1, n - shown_rows);
        fill(layer, {box.x2 - 5, thumb_y, box.x2 - 2, thumb_y + thumb_h - 1}, lv_color_hex(c_.dim), 2);
    }
}

} // namespace screen
