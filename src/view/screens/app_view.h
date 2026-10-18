/*
 * APP as graphics: the settings as the home menu draws
 * its rows (icon, label, the value on the right): the font size, the theme
 * with its colours, the spectrum as a switch, the language with its flag.
 * The instructions follow, wrapped by LVGL in pixels (so Japanese and Chinese
 * wrap too), and the whole page scrolls up as one column. One LVGL object,
 * one draw callback.
 */
#pragma once

#include "i18n/i18n.h"
#include "lvgl.h"

#include <array>
#include <cstdint>
#include <string>
#include <vector>

namespace screen {

class AppView {
public:
    struct Colors {
        uint32_t bg, panel, fg, dim, hi, accent, cursor, on_cursor;
    };
    struct Fonts {
        const lv_font_t* label;
        const lv_font_t* small;
        const lv_font_t* icon;
    };
    struct State {
        std::array<std::string, 4> labels;  // FONT SIZE, THEME, SPECTRUM, LANGUAGE
        int font_size{14};
        std::string theme;
        std::array<uint32_t, 5> swatches{};  // the theme's colours
        bool spectrum{true};
        zt::i18n::Lang lang{zt::i18n::Lang::En};
        std::string help_title;
        std::vector<std::string> help;
        int selected{0};  // -1 while reading the instructions
        int scroll{0};    // px
        std::string version;
    };

    void create(lv_obj_t* parent, int x, int y, int w, int h, const Fonts& fonts, const Colors& colors);
    void set_visible(bool visible);
    void set(State state);
    // Height of the whole column (settings and instructions) in px, for the
    // scroll limit; the visible height.
    int content_height() const;
    int height() const { return h_; }

private:
    static void draw_cb(lv_event_t* e);
    void draw(lv_layer_t* layer, const lv_area_t& box) const;

    lv_obj_t* obj_{nullptr};
    Fonts f_{};
    Colors c_{};
    int w_{0}, h_{0};
    State s_;
};

} // namespace screen
