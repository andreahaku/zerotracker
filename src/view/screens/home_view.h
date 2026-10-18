/*
 * Home screen: the app's logo, its name and three
 * choices, RESUME (the song of the last session), LOAD (the song browser)
 * and NEW. The logo is a ring, the zero, with eight bars inside, one per
 * track, that move gently. One LVGL object, one draw callback.
 */
#pragma once

#include "lvgl.h"

#include <array>
#include <cstdint>
#include <string>

namespace screen {

class HomeView {
public:
    struct Colors {
        uint32_t bg, panel, fg, dim, hi, accent, cursor, on_cursor;
        std::array<uint32_t, 5> chain;  // the bars, left to right
    };
    struct Item {
        const char* icon;    // a Phosphor glyph
        std::string label;   // RESUME, LOAD, NEW
        std::string detail;  // e.g. the song RESUME opens
        bool enabled;
    };
    struct Fonts {
        const lv_font_t* title;
        const lv_font_t* label;
        const lv_font_t* small;
        const lv_font_t* icon;
    };

    void create(lv_obj_t* parent, int x, int y, int w, int h, const Fonts& fonts,
                const Colors& colors, const char* version);
    void set_visible(bool visible);
    // The line under the name, in the APP language.
    void set_tagline(const char* tagline);
    bool visible() const;
    // `selected` -1: no row on a pill.
    void set(const std::array<Item, 4>& items, int selected);
    // Moves the logo's bars; redraws the logo only.
    void animate(uint32_t now_ms);

private:
    static void draw_cb(lv_event_t* e);
    void draw(lv_layer_t* layer, const lv_area_t& box) const;
    void draw_logo(lv_layer_t* layer, int x, int y) const;

    lv_obj_t* obj_{nullptr};
    Fonts f_{};
    Colors c_{};
    std::string version_;
    std::string tagline_{"pocket music tracker"};
    int w_{0}, h_{0};
    std::array<Item, 4> items_{};
    int selected_{0};
    std::array<float, 8> bars_{};  // 0..1
};

} // namespace screen
