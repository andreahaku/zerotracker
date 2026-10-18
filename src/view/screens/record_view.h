/*
 * REC. (the SAMPLER's record, as the M8's sample editor records): the
 * microphone icon, the time in large type, a level meter and the waveform as
 * it comes in. Keys: ENTER or 5 START/STOP, ESC or 8 CANCEL. One LVGL
 * object, one draw callback.
 */
#pragma once

#include "lvgl.h"

#include <cstdint>
#include <string>
#include <vector>

namespace screen {

class RecordView {
public:
    struct Colors {
        uint32_t bg, panel, fg, dim, hi, accent, alert;
    };
    struct Fonts {
        const lv_font_t* big;
        const lv_font_t* label;
        const lv_font_t* small;
        const lv_font_t* icon;
    };
    struct State {
        bool recording{false};
        double seconds{0};
        float level{0};             // 0..1
        std::vector<float> peaks;   // the waveform so far, 0..1
        std::string title, status, hint, error;
    };

    void create(lv_obj_t* parent, int x, int y, int w, int h, const Fonts& fonts, const Colors& colors);
    void set_visible(bool visible);
    void set(State state);

private:
    static void draw_cb(lv_event_t* e);
    void draw(lv_layer_t* layer, const lv_area_t& box) const;

    lv_obj_t* obj_{nullptr};
    Fonts f_{};
    Colors c_{};
    State s_;
};

} // namespace screen
