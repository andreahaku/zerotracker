/*
 * SONG as graphics (out of the text-only look, with the
 * home screen's icons and rows). Left: the name, the tempo in large type and
 * the transpose; right: SAVE, NEW and LOAD as the home menu draws its rows;
 * at the bottom what the song uses. RENDER writes the song to WAV files. The selected field sits on a pill, in the
 * alert colour while it is being edited. One LVGL object, one draw callback.
 */
#pragma once

#include "lvgl.h"
#include "zt_viewer.h"

#include <cstdint>
#include <string>

namespace screen {

class SongView {
public:
    struct Colors {
        uint32_t bg, panel, fg, dim, hi, accent, cursor, on_cursor, alert;
    };
    struct Fonts {
        const lv_font_t* big;    // the tempo
        const lv_font_t* label;  // names and values
        const lv_font_t* small;  // captions and the bottom line
        const lv_font_t* icon;   // Phosphor
    };
    // In the APP language.
    struct Words {
        std::string save, fresh, load, unsaved, rows, chains, phrases, instruments, render;
    };

    void create(lv_obj_t* parent, int x, int y, int w, int h, const Fonts& fonts,
                const Colors& colors);
    void set_visible(bool visible);
    // `name_override`: the NAME being typed, when not empty.
    void set(const ZtSongPage& page, const Words& words, const std::string& name_override);
    // RENDER's progress or result beside it ("40%", "DONE").
    void set_render_status(const std::string& status, bool failed);

private:
    static void draw_cb(lv_event_t* e);
    void draw(lv_layer_t* layer, const lv_area_t& box) const;

    lv_obj_t* obj_{nullptr};
    Fonts f_{};
    Colors c_{};
    ZtSongPage page_{};
    Words words_;
    std::string name_;
    std::string render_status_;
    bool render_failed_{false};
};

} // namespace screen
