/*
 * System output volume (PipeWire default sink) through wpctl. Device only:
 * on the desktop these calls do nothing, so the development machine's
 * volume is left alone.
 */
#pragma once

#include <string>

namespace platform::volume {

enum class Change { ToggleMute, Down, Up };

// Applies the change (5% steps, capped at 100%) and returns the new state
// as text for the top bar, e.g. "VOL 85%" or "MUTED". Empty on failure.
std::string apply(Change change);

} // namespace platform::volume
