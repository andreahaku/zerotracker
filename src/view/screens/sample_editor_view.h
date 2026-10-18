/*
 * Sample editor: the waveform of the sample across the
 * screen with the selection shaded, the loop region as a bracket, the slice
 * markers numbered, the active marker bright and the playhead; a header
 * with the file and its format, a footer with the positions, and an
 * overview strip that shows which part a zoom looks at. One LVGL object,
 * one draw callback.
 */
#pragma once

#include "lvgl.h"

#include <array>
#include <cstdint>
#include <string>
#include <vector>

namespace screen {

class SampleEditorView {
public:
    struct Colors {
        uint32_t bg, panel, fg, dim, hi, accent, selection, loop, slice, playhead, wave;
    };
    // What to draw; the screen fills it on every change.
    struct State {
        std::string title;    // file name
        std::string format;   // "44.1k  stereo  16 bit  1.23 s"
        std::string mode;     // SELECT / LOOP / SLICE
        std::string footer;   // positions of the active markers
        uint32_t frames{0};
        uint32_t view_from{0}, view_to{0};
        std::vector<float> lo, hi;  // one pair per column of the view
        uint32_t sel_a{0}, sel_b{0};
        bool has_loop{false};
        uint32_t loop_a{0}, loop_b{0};
        std::vector<uint32_t> slices;
        int64_t marker{-1};    // the active marker (frame), bright
        int64_t playhead{-1};  // frame being heard, or -1
    };

    void create(lv_obj_t* parent, int x, int y, int w, int h, const lv_font_t* font,
                const lv_font_t* bold, const lv_font_t* small, const Colors& colors);
    void set_visible(bool visible);
    bool visible() const;
    void set(State state);
    // The playhead moves on its own: no redraw of the waveform (which would
    // take the CPU from the audio while a preview plays). -1 hides it.
    void set_playhead(int64_t frame);
    // Width of the waveform in columns (the screen asks for that many peaks).
    int columns() const { return w_ - 2 * kMargin; }

    static constexpr int kMargin = 4;

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
    lv_obj_t* playhead_{nullptr};
    int wave_top_{0}, wave_bottom_{0};
};

} // namespace screen
