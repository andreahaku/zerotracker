/*
 * EqEditorView. See eq_editor_view.h.
 */
#include "eq_editor_view.h"

#include <algorithm>
#include <chrono>
#include <cstdlib>
#include <cmath>
#include <cstdio>

namespace screen {
namespace {

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

} // namespace

void EqEditorView::create(lv_obj_t* parent, int x, int y, int w, int h, const lv_font_t* font,
                          const lv_font_t* bold, const lv_font_t* small, const Colors& colors) {
    obj_ = lv_obj_create(parent);
    // ZT_PERF=1: the display's refresh time too, start to ready.
    if (std::getenv("ZT_PERF")) {
        static std::chrono::steady_clock::time_point start;
        lv_display_add_event_cb(
            lv_display_get_default(),
            [](lv_event_t* e) {
                static double total = 0;
                static int frames = 0;
                if (lv_event_get_code(e) == LV_EVENT_REFR_START) {
                    start = std::chrono::steady_clock::now();
                    return;
                }
                if (lv_event_get_code(e) != LV_EVENT_REFR_READY) return;
                total += std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - start).count();
                if (++frames == 30) {
                    std::fprintf(stderr, "[perf] refresh %.2f ms\n", total / frames);
                    total = 0;
                    frames = 0;
                }
            },
            LV_EVENT_ALL, nullptr);
    }
    lv_obj_remove_style_all(obj_);
    lv_obj_set_pos(obj_, x, y);
    lv_obj_set_size(obj_, w, h);
    lv_obj_remove_flag(obj_, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(obj_, draw_cb, LV_EVENT_DRAW_MAIN, this);
    font_ = font;
    bold_ = bold ? bold : font;
    small_ = small ? small : font;
    c_ = colors;
    w_ = w;
    h_ = h;
}

void EqEditorView::set_visible(bool visible) {
    if (!obj_) return;
    if (visible) lv_obj_remove_flag(obj_, LV_OBJ_FLAG_HIDDEN);
    else lv_obj_add_flag(obj_, LV_OBJ_FLAG_HIDDEN);
}

void EqEditorView::set(State state) {
    s_ = std::move(state);
    if (obj_) lv_obj_invalidate(obj_);
}

void EqEditorView::draw_cb(lv_event_t* e) {
    auto* self = static_cast<EqEditorView*>(lv_event_get_user_data(e));
    lv_area_t box;
    lv_obj_get_coords(self->obj_, &box);
    // ZT_PERF=1 prints how long the frames take to queue (about every 2 s).
    static const bool perf = std::getenv("ZT_PERF") != nullptr;
    const auto t0 = std::chrono::steady_clock::now();
    self->draw(lv_event_get_layer(e), box);
    if (perf) {
        static double total = 0;
        static int frames = 0;
        total += std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - t0).count();
        if (++frames == 30) {
            std::fprintf(stderr, "[perf] EQ draw %.2f ms\n", total / frames);
            total = 0;
            frames = 0;
        }
    }
}

void EqEditorView::draw(lv_layer_t* layer, const lv_area_t& box) const {
    fill(layer, box, lv_color_hex(c_.bg));
    const int bh = lv_font_get_line_height(bold_);
    const int sh = lv_font_get_line_height(small_);
    const Band& sel = s_.bands[static_cast<size_t>(std::clamp(s_.selected, 0, 2))];
    const lv_color_t sel_color = lv_color_hex(c_.band[static_cast<size_t>(std::clamp(s_.selected, 0, 2))]);

    // Header: the EQ and the instrument, the selected band on a pill.
    text(layer, {box.x1 + 4, box.y1 + 1, box.x2 - 70, box.y1 + bh}, s_.title.c_str(), bold_,
         lv_color_hex(c_.hi));
    char pill[24];
    std::snprintf(pill, sizeof pill, "%d %s", s_.selected + 1, sel.name.c_str());
    lv_point_t ps;
    lv_text_get_size(&ps, pill, small_, 0, 0, LV_COORD_MAX, LV_TEXT_FLAG_NONE);
    const lv_area_t pa{box.x2 - ps.x - 12, box.y1 + 2, box.x2 - 2, box.y1 + 2 + sh + 2};
    fill(layer, pa, sel_color, 4);
    text(layer, {pa.x1, pa.y1 + 1, pa.x2, pa.y2}, pill, small_, lv_color_hex(c_.bg), LV_TEXT_ALIGN_CENTER);

    // Graph: log frequency across, dB up; the range grows with the curve.
    const int gx0 = box.x1 + kLeft, gx1 = box.x2 - kRight;
    const int gy0 = box.y1 + bh + 3, gy1 = box.y2 - 2 * sh - 6;
    fill(layer, {gx0, gy0, gx1, gy1}, lv_color_hex(c_.panel), 3);
    double peak = 12.0;
    for (float v : s_.total) peak = std::max(peak, std::fabs(double(v)) + 3.0);
    for (const Band& b : s_.bands) peak = std::max(peak, std::fabs(b.gain) + 3.0);
    const double range = std::min(48.0, std::ceil(peak / 6.0) * 6.0);
    const int cols = gx1 - gx0 + 1;
    auto x_of = [&](double hz) {
        const double t = std::log(std::clamp(hz, kMinHz, kMaxHz) / kMinHz) / std::log(kMaxHz / kMinHz);
        return gx0 + static_cast<int>(std::lround(t * (cols - 1)));
    };
    auto y_of = [&](double db) {
        const double t = std::clamp(db, -range, range) / range;
        return (gy0 + gy1) / 2 - static_cast<int>(std::lround(t * ((gy1 - gy0) / 2 - 2)));
    };
    // The instrument's spectrum behind everything else, as a line 2 px thick
    // (filled it drew too slowly on the device): a run
    // of equal height is one rectangle, a step a short opaque segment.
    {
        const int n = std::min(cols, static_cast<int>(s_.spectrum.size()));
        const int gh = gy1 - gy0 - 3;
        auto top = [&](int i) { return gy1 - 2 - static_cast<int>(s_.spectrum[i] * gh); };
        const lv_color_t color = lv_color_hex(c_.accent);
        int prev = top(0);
        for (int i = 0; i < n;) {
            const int y = top(i);
            int j = i + 1;
            while (j < n && top(j) == y) ++j;
            // The step from the previous run, then the run itself.
            if (i > 0 && y != prev) fill(layer, {gx0 + i, std::min(y, prev), gx0 + i + 1, std::max(y, prev) + 1}, color);
            if (y < gy1 - 2) fill(layer, {gx0 + i, y, gx0 + j - 1, y + 1}, color);
            prev = y;
            i = j;
        }
    }
    // Grid: decades and 0 dB, with their labels.
    for (double hz : {100.0, 1000.0, 10000.0}) {
        const int x = x_of(hz);
        fill(layer, {x, gy0 + 1, x, gy1 - 1}, lv_color_mix(lv_color_hex(c_.dim), lv_color_hex(c_.panel), LV_OPA_30));
        const char* label = hz < 1000 ? "100" : hz < 10000 ? "1K" : "10K";
        text(layer, {x - 15, gy1 - sh, x + 15, gy1}, label, small_, lv_color_hex(c_.dim), LV_TEXT_ALIGN_CENTER);
    }
    for (int k = -1; k <= 1; ++k) {
        const double db = k * range / 2;
        const int y = y_of(db);
        fill(layer, {gx0 + 1, y, gx1 - 1, y},
             lv_color_mix(lv_color_hex(c_.dim), lv_color_hex(c_.panel), k == 0 ? LV_OPA_60 : LV_OPA_20));
        char s[8];
        std::snprintf(s, sizeof s, "%+d", static_cast<int>(db));
        text(layer, {box.x1, y - sh / 2, gx0 - 3, y + sh / 2}, k == 0 ? "0" : s, small_,
             lv_color_hex(c_.dim), LV_TEXT_ALIGN_RIGHT);
    }

    // Each band's own curve, faint, then the sum, bright. Opaque runs and
    // steps instead of antialiased segments, as the spectrum: one draw per
    // flat run (the EQ redraws while it plays; segments were too slow on the
    // device). The faint colours are mixed with the graph's ahead.
    auto curve = [&](const std::vector<float>& v, lv_color_t color, int width) {
        const int n = std::min(cols, static_cast<int>(v.size()));
        int prev = n > 0 ? y_of(v[0]) : 0;
        for (int i = 0; i < n;) {
            const int y = y_of(v[i]);
            int j = i + 1;
            while (j < n && y_of(v[j]) == y) ++j;
            if (i > 0 && y != prev) {
                fill(layer, {gx0 + i, std::min(y, prev), gx0 + i + width - 1, std::max(y, prev) + width - 1}, color);
            }
            fill(layer, {gx0 + i, y, gx0 + j - 1, y + width - 1}, color);
            prev = y;
            i = j;
        }
    };
    const lv_color_t graph = lv_color_hex(c_.panel);
    for (size_t b = 0; b < 3; ++b) {
        const lv_opa_t opa = b == size_t(s_.selected) ? LV_OPA_80 : LV_OPA_40;
        curve(s_.band_curve[b], lv_color_mix(lv_color_hex(c_.band[b]), graph, opa), 1);
    }
    curve(s_.total, lv_color_hex(c_.hi), 2);

    // Handles: at the band's frequency and gain (on 0 dB for the bands
    // without one); the selected one larger, with a ring.
    for (size_t b = 0; b < 3; ++b) {
        const Band& band = s_.bands[b];
        const int x = x_of(band.freq);
        const int y = y_of(band.has_gain ? band.gain : 0.0);
        const bool on = int(b) == s_.selected;
        const int r = on ? 7 : 5;
        if (on) fill(layer, {x - r - 2, y - r - 2, x + r + 2, y + r + 2}, lv_color_hex(c_.hi), LV_RADIUS_CIRCLE);
        fill(layer, {x - r, y - r, x + r, y + r}, lv_color_hex(c_.band[b]), LV_RADIUS_CIRCLE);
        char n[2] = {char('1' + b), 0};
        text(layer, {x - r, y - sh / 2, x + r, y + sh / 2}, n, small_, lv_color_hex(c_.bg), LV_TEXT_ALIGN_CENTER);
    }

    // Footer: the selected band spelled out.
    char a[64], b[64];
    std::snprintf(a, sizeof a, "%s  %s", sel.kind.c_str(), sel.mode.c_str());
    if (sel.has_gain) {
        std::snprintf(b, sizeof b, "%.0f Hz   %+.2f dB   Q %d", sel.freq, sel.gain, sel.q);
    }
    else {
        std::snprintf(b, sizeof b, "%.0f Hz   Q %d", sel.freq, sel.q);
    }
    const int fy = gy1 + 3;
    text(layer, {box.x1 + 4, fy, box.x2 - 4, fy + sh}, a, small_, sel_color);
    text(layer, {box.x1 + 4, fy + sh + 1, box.x2 - 4, fy + 2 * sh + 1}, b, small_, lv_color_hex(c_.fg));
}

} // namespace screen
