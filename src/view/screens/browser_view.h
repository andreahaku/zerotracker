/*
 * LOAD: the file browser, opened from the home
 * screen. A header with the folder and the item count, then one row per
 * entry with an icon for its kind (folder, song, instrument, sample), its
 * name and its size; the selected row on a pill, a scroll bar on the right.
 */
#pragma once

#include "lvgl.h"

#include <cstdint>
#include <string>
#include <vector>

namespace screen {

class BrowserView {
public:
    enum class Kind { Parent, Folder, Song, Instrument, Sample };
    struct Row {
        Kind kind;
        std::string name;
        std::string detail;  // size, or empty
        bool enabled;        // grey when it cannot be opened in this browser
        // LIBRARY: 1 installed (a check and the detail in the accent colour),
        // 2 not installed (a download cloud); 0 a file row.
        int mark{0};
        std::string sub;  // a second, dim line under the name (two-line lists)
    };
    struct Colors {
        uint32_t bg, panel, fg, dim, hi, accent, cursor, on_cursor;
        uint32_t folder, song, instrument, sample;
        uint32_t panel_text;  // rows that cannot be opened here
    };
    struct Fonts {
        const lv_font_t* header;
        const lv_font_t* row;
        const lv_font_t* small;
        const lv_font_t* icon;
    };

    void create(lv_obj_t* parent, int x, int y, int w, int h, const Fonts& fonts,
                const Colors& colors);
    void set_visible(bool visible);
    // "N items" and the empty-folder line, in the APP language.
    void set_words(const char* items, const char* empty);
    // A line under the header saying what the list is (LIBRARY); none when empty.
    void set_note(const std::string& note);
    // Rows of two lines: the name, then `sub` (LIBRARY).
    void set_two_line(bool on) { two_line_ = on; }
    // Rows that fit under the header (and the note).
    int visible_rows() const;
    void set(const std::string& path, const std::vector<Row>& rows, int selected, int top,
             const std::string& message);

private:
    static void draw_cb(lv_event_t* e);
    void draw(lv_layer_t* layer, const lv_area_t& box) const;

    lv_obj_t* obj_{nullptr};
    Fonts f_{};
    Colors c_{};
    int w_{0}, h_{0};
    std::string path_, message_, note_;
    int note_h() const;
    int row_h() const;
    bool two_line_{false};
    std::string items_word_{"items"}, empty_text_{"no songs here"};
    std::vector<Row> rows_;
    int selected_{0}, top_{0};
    bool valid_{false};
};

} // namespace screen
