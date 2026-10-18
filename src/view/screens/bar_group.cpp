/*
 * BarGroup. See bar_group.h.
 */
#include "bar_group.h"

#include <algorithm>

namespace screen {

void BarGroup::create(lv_obj_t* parent, int x, int y, int w, int h, uint32_t color) {
    obj_ = lv_obj_create(parent);
    lv_obj_remove_style_all(obj_);
    lv_obj_set_pos(obj_, x, y);
    lv_obj_set_size(obj_, w, h);
    lv_obj_remove_flag(obj_, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(obj_, draw_cb, LV_EVENT_DRAW_MAIN, this);
    h_ = h;
    color_ = color;
    x_.clear();
    width_.clear();
    heights_.clear();
}

void BarGroup::set_area(int x, int y, int w, int h) {
    if (!obj_) return;
    lv_obj_set_pos(obj_, x, y);
    lv_obj_set_size(obj_, w, h);
    h_ = h;
}

void BarGroup::set_layout(std::vector<int> x, std::vector<int> width) {
    x_ = std::move(x);
    width_ = std::move(width);
    heights_.assign(x_.size(), 0);
    if (obj_) lv_obj_invalidate(obj_);
}

void BarGroup::set_heights(const std::vector<int>& heights) {
    bool changed = false;
    for (size_t i = 0; i < heights_.size() && i < heights.size(); ++i) {
        const int h = std::clamp(heights[i], 0, h_);
        if (h != heights_[i]) {
            heights_[i] = h;
            changed = true;
        }
    }
    if (changed && obj_) lv_obj_invalidate(obj_);
}

void BarGroup::set_visible(bool visible) {
    if (!obj_) return;
    if (visible) lv_obj_remove_flag(obj_, LV_OBJ_FLAG_HIDDEN);
    else lv_obj_add_flag(obj_, LV_OBJ_FLAG_HIDDEN);
}

void BarGroup::draw_cb(lv_event_t* e) {
    auto* self = static_cast<BarGroup*>(lv_event_get_user_data(e));
    lv_layer_t* layer = lv_event_get_layer(e);
    lv_area_t box;
    lv_obj_get_coords(self->obj_, &box);
    lv_draw_rect_dsc_t dsc;
    lv_draw_rect_dsc_init(&dsc);
    dsc.bg_color = lv_color_hex(self->color_);
    dsc.bg_opa = LV_OPA_COVER;
    for (size_t i = 0; i < self->heights_.size(); ++i) {
        const int h = self->heights_[i];
        if (h <= 0) continue;
        lv_area_t a;
        a.x1 = box.x1 + self->x_[i];
        a.x2 = a.x1 + self->width_[i] - 1;
        a.y2 = box.y2;
        a.y1 = box.y2 - h + 1;
        lv_draw_rect(layer, &dsc, &a);
    }
}

} // namespace screen
