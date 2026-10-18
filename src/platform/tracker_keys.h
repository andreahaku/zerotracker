/*
 * Held-key state for the tracker: M8 buttons need chords (SHIFT+arrow,
 * EDIT+arrow), which the Template's single-key router does not track.
 *
 * Mapping: arrows = Cardputer F/X/Z/C or the arrow keys; PLAY = SPACE,
 * SHIFT = M, EDIT = L, OPTION = K. Key 4 switches the page; keys 5-8 are the
 * page's bottom-bar buttons (on page 1: OPTION, EDIT, SHIFT, PLAY).
 * ESC, DEL (undo) and Fn+DEL (redo) are reported apart.
 */
#pragma once

#include <cstdint>
#include <string>

namespace platform::tracker_keys {

struct State {
    uint32_t buttons{0};  // ZT_BTN_* bits currently held
    bool escape{false};   // ESC currently held
    bool page{false};     // 4 held: next page
    uint32_t soft{0};     // keys 5-8 held, bits 0-3
    // Text entry: the characters whose keys are held (A-Z 0-9 space - .),
    // and ENTER.
    std::string chars;
    bool enter{false};
    bool undo{false};     // DEL (Backspace) held
    bool redo{false};     // Fn+DEL (Delete) held
    bool ctrl{false};     // Ctrl held: Ctrl+Z / Y / S / C / V shortcuts
    bool sym{false};      // SYM held (Left Alt on the desktop): SYM+4..8 explain the keys
    // Volume keys (the Cardputer sends KEY_MUTE / KEY_VOLUMEDOWN / KEY_VOLUMEUP).
    bool mute{false};
    bool volume_down{false};
    bool volume_up{false};
};

// Opens the keyboard on the device (no-op on desktop). Safe to call twice.
void init();

// Reads pending key events and returns the current held state.
State poll();

} // namespace platform::tracker_keys
