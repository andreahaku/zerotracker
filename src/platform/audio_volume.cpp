/*
 * System output volume. See audio_volume.h.
 */
#include "audio_volume.h"

#include <cstdio>
#include <cstdlib>

namespace platform::volume {

std::string apply(Change change) {
#if USE_DESKTOP
    (void)change;
    return {};
#else
    const char* cmd = nullptr;
    switch (change) {
        case Change::ToggleMute: cmd = "wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle"; break;
        case Change::Down: cmd = "wpctl set-volume @DEFAULT_AUDIO_SINK@ 5%-"; break;
        case Change::Up: cmd = "wpctl set-volume -l 1.0 @DEFAULT_AUDIO_SINK@ 5%+"; break;
    }
    // Launched from APPLaunch the session bus and runtime dir are set; from
    // ssh they may not be.
    setenv("XDG_RUNTIME_DIR", "/run/user/1000", 0);
    if (std::system(cmd) != 0) {
        return {};
    }
    // "Volume: 0.85" or "Volume: 0.85 [MUTED]"
    std::string out;
    if (FILE* p = popen("wpctl get-volume @DEFAULT_AUDIO_SINK@", "r")) {
        char buf[64] = {};
        if (fgets(buf, sizeof(buf), p)) {
            out = buf;
        }
        pclose(p);
    }
    if (out.find("MUTED") != std::string::npos) {
        return "MUTED";
    }
    const auto colon = out.find(':');
    if (colon == std::string::npos) {
        return {};
    }
    const int percent = static_cast<int>(std::atof(out.c_str() + colon + 1) * 100.0 + 0.5);
    return "VOL " + std::to_string(percent) + "%";
#endif
}

} // namespace platform::volume
