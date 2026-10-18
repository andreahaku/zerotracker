/*
 * EQ editor: the instrument's three-band EQ as a
 * response curve from 20 Hz to 20 kHz instead of the M8's table. Each band
 * has a numbered handle in its colour and a faint curve of its own; the
 * sum is drawn bright. The selected band's values are spelled out below.
 * While the song plays, the spectrum of the tracks playing this instrument
 * (summed, not the master) fills the graph
 * from the bottom, so the peaks to cut or boost are in view.
 * One LVGL object, one draw callback.
 */
#pragma once

#include "lvgl.h"

#include <array>
#include <cstdint>
#include <string>
#include <vector>

namespace screen {

class EqEditorView {
public:
    struct Colors {
        uint32_t bg, panel, fg, dim, hi, accent;
        std::array<uint32_t, 3> band;
    };
    struct Band {
        std::string name;    // LOW / MID / HIGH
        std::string kind;    // LOWSHELF ...
        std::string mode;    // STEREO ...
        double freq{0};      // Hz
        double gain{0};      // dB
        int q{50};
        bool has_gain{true};  // cuts, band-pass and all-pass ignore GAIN
    };
    // What to draw; the screen fills it on every change.
    struct State {
        std::string title;   // "EQ 11  INST 11 KICK"
        std::array<Band, 3> bands;
        int selected{0};
        std::vector<float> total;                 // dB, one per column
        std::array<std::vector<float>, 3> band_curve;
        // Master output while playing, 0..1 (-80..-8 dBFS) per column; empty
        // when there is nothing to show.
        std::vector<float> spectrum;
    };

    void create(lv_obj_t* parent, int x, int y, int w, int h, const lv_font_t* font,
                const lv_font_t* bold, const lv_font_t* small, const Colors& colors);
    void set_visible(bool visible);
    void set(State state);
    // Width of the graph in columns (the screen asks for that many points).
    int columns() const { return w_ - kLeft - kRight; }

    static constexpr double kMinHz = 20.0, kMaxHz = 20000.0;

    static constexpr int kLeft = 26;   // room for the dB scale
    static constexpr int kRight = 4;

private:
    static void draw_cb(lv_event_t* e);
    void draw(lv_layer_t* layer, const lv_area_t& box) const;

    lv_obj_t* obj_{nullptr};
    const lv_font_t* font_{nullptr};
    const lv_font_t* bold_{nullptr};
    const lv_font_t* small_{nullptr};
    Colors c_{};
    int w_{0}, h_{0};
    State s_;
};

} // namespace screen
