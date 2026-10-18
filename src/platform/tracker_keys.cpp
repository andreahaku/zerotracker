/*
 * Held-key state for the tracker. See tracker_keys.h.
 */
#include "tracker_keys.h"

#include "zt_viewer.h"

#include <cstdio>
#include <cstdlib>

#if USE_DESKTOP
#include <SDL2/SDL.h>
#else
#include <fcntl.h>
#include <linux/input.h>
#include <unistd.h>

#include <bitset>
#include <string>
#endif

namespace platform::tracker_keys {
namespace {

#if USE_DESKTOP

void init_impl() {}

State poll_impl() {
    State s;
    const Uint8* k = SDL_GetKeyboardState(nullptr);
    if (!k) {
        return s;
    }
    auto held = [&](SDL_Scancode a, SDL_Scancode b = SDL_SCANCODE_UNKNOWN) {
        return k[a] || (b != SDL_SCANCODE_UNKNOWN && k[b]);
    };
    if (held(SDL_SCANCODE_F, SDL_SCANCODE_UP)) s.buttons |= ZT_BTN_UP;
    if (held(SDL_SCANCODE_X, SDL_SCANCODE_DOWN)) s.buttons |= ZT_BTN_DOWN;
    if (held(SDL_SCANCODE_Z, SDL_SCANCODE_LEFT)) s.buttons |= ZT_BTN_LEFT;
    if (held(SDL_SCANCODE_C, SDL_SCANCODE_RIGHT)) s.buttons |= ZT_BTN_RIGHT;
    if (held(SDL_SCANCODE_M)) s.buttons |= ZT_BTN_SHIFT;
    if (held(SDL_SCANCODE_SPACE)) s.buttons |= ZT_BTN_PLAY;
    if (held(SDL_SCANCODE_K)) s.buttons |= ZT_BTN_OPTION;
    if (held(SDL_SCANCODE_L)) s.buttons |= ZT_BTN_EDIT;
    s.page = held(SDL_SCANCODE_4);
    s.sym = held(SDL_SCANCODE_LALT);
    const SDL_Scancode soft[4] = {SDL_SCANCODE_5, SDL_SCANCODE_6, SDL_SCANCODE_7, SDL_SCANCODE_8};
    for (int i = 0; i < 4; ++i) {
        if (held(soft[i])) s.soft |= 1u << i;
    }
    for (int i = 0; i < 26; ++i) {
        if (held(static_cast<SDL_Scancode>(SDL_SCANCODE_A + i))) s.chars += static_cast<char>('A' + i);
    }
    for (int i = 0; i < 10; ++i) {
        // SDL orders 1..9 then 0.
        if (held(static_cast<SDL_Scancode>(SDL_SCANCODE_1 + i))) s.chars += static_cast<char>(i == 9 ? '0' : '1' + i);
    }
    if (held(SDL_SCANCODE_SPACE)) s.chars += ' ';
    if (held(SDL_SCANCODE_MINUS)) s.chars += '-';
    if (held(SDL_SCANCODE_PERIOD)) s.chars += '.';
    s.enter = held(SDL_SCANCODE_RETURN);
    s.escape = held(SDL_SCANCODE_ESCAPE);
    s.undo = held(SDL_SCANCODE_BACKSPACE);
    s.redo = held(SDL_SCANCODE_DELETE);
    s.ctrl = held(SDL_SCANCODE_LCTRL, SDL_SCANCODE_RCTRL);
    return s;
}

#else

int g_fd = -1;
std::bitset<KEY_MAX + 1> g_down;

void init_impl() {
    if (g_fd >= 0) {
        return;
    }
    const char* env = std::getenv("ZT_KEYBOARD_DEVICE");
    const std::string path = env && *env ? env : "/dev/input/cardputer-zero-internal";
    g_fd = open(path.c_str(), O_RDONLY | O_NONBLOCK | O_CLOEXEC);
}

State poll_impl() {
    if (g_fd >= 0) {
        input_event ev{};
        while (read(g_fd, &ev, sizeof(ev)) == sizeof(ev)) {
            if (ev.type == EV_KEY && ev.code <= KEY_MAX && ev.value != 2) {
                g_down[ev.code] = ev.value != 0;
                static const bool log = std::getenv("ZT_KEYLOG") != nullptr;
                if (log) {
                    std::fprintf(stderr, "[keys] code %u %s\n", ev.code, ev.value ? "down" : "up");
                }
            }
        }
    }
    State s;
    auto held = [&](int a, int b = -1) { return g_down[a] || (b >= 0 && g_down[b]); };
    if (held(KEY_F, KEY_UP)) s.buttons |= ZT_BTN_UP;
    if (held(KEY_X, KEY_DOWN)) s.buttons |= ZT_BTN_DOWN;
    if (held(KEY_Z, KEY_LEFT)) s.buttons |= ZT_BTN_LEFT;
    if (held(KEY_C, KEY_RIGHT)) s.buttons |= ZT_BTN_RIGHT;
    if (held(KEY_M)) s.buttons |= ZT_BTN_SHIFT;
    if (held(KEY_SPACE)) s.buttons |= ZT_BTN_PLAY;
    if (held(KEY_K)) s.buttons |= ZT_BTN_OPTION;
    if (held(KEY_L)) s.buttons |= ZT_BTN_EDIT;
    s.page = held(KEY_4);
    s.sym = held(KEY_CD);  // the Cardputer's SYM key
    const int soft[4] = {KEY_5, KEY_6, KEY_7, KEY_8};
    for (int i = 0; i < 4; ++i) {
        if (held(soft[i])) s.soft |= 1u << i;
    }
    // With SYM held the symbol layer sends 4..8 as these codes (as ZeroRadio
    // reads them): they stay keys 4..8, for the SYM+4..8 tooltips.
    if (s.sym) {
        if (held(KEY_APOSTROPHE)) s.page = true;
        const int sym_soft[4] = {KEY_GRAVE, KEY_BACKSLASH, KEY_COMMA, KEY_DOT};
        for (int i = 0; i < 4; ++i) {
            if (held(sym_soft[i])) s.soft |= 1u << i;
        }
    }
    // Linux key codes are in keyboard order, not alphabetical.
    static const int letters[26] = {KEY_A, KEY_B, KEY_C, KEY_D, KEY_E, KEY_F, KEY_G, KEY_H, KEY_I,
                                    KEY_J, KEY_K, KEY_L, KEY_M, KEY_N, KEY_O, KEY_P, KEY_Q, KEY_R,
                                    KEY_S, KEY_T, KEY_U, KEY_V, KEY_W, KEY_X, KEY_Y, KEY_Z};
    for (int i = 0; i < 26; ++i) {
        if (held(letters[i])) s.chars += static_cast<char>('A' + i);
    }
    for (int i = 0; i < 10; ++i) {
        // KEY_1..KEY_9 then KEY_0.
        if (held(KEY_1 + i)) s.chars += static_cast<char>(i == 9 ? '0' : '1' + i);
    }
    if (held(KEY_SPACE)) s.chars += ' ';
    if (held(KEY_MINUS)) s.chars += '-';
    // The "." key arrives as KEY_HIRAGANA; KEY_DOT is SYM+8.
    if (held(KEY_HIRAGANA)) s.chars += '.';
    s.enter = held(KEY_ENTER);
    s.escape = held(KEY_ESC);
    // Assumed codes for the Cardputer DEL and Fn+DEL; ZT_KEYLOG=1 prints the
    // codes to check them on the device.
    s.undo = held(KEY_BACKSPACE);
    s.redo = held(KEY_DELETE);
    s.ctrl = held(KEY_LEFTCTRL, KEY_RIGHTCTRL);
    s.mute = held(KEY_MUTE);
    s.volume_down = held(KEY_VOLUMEDOWN);
    s.volume_up = held(KEY_VOLUMEUP);
    return s;
}

#endif

} // namespace

void init() {
    init_impl();
}

State poll() {
    return poll_impl();
}

} // namespace platform::tracker_keys
