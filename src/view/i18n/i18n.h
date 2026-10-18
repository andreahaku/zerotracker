/*
 * SPDX-FileCopyrightText: 2026 One Small Step Apps Ltd
 *
 * SPDX-License-Identifier: MIT
 *
 * Ported from ZeroRadio's toolkit (17/10/2026).
 */

#pragma once

namespace zt::i18n {

// UI languages with translations. Chinese is Simplified for every zh locale.
enum class Lang { En, It, Fr, De, Es, Ja, Zh };

// One string in every supported language. A null entry falls back to `en`.
struct Text {
    const char* en{nullptr};
    const char* it{nullptr};
    const char* fr{nullptr};
    const char* de{nullptr};
    const char* es{nullptr};
    const char* ja{nullptr};
    const char* zh{nullptr};

    bool empty() const { return en == nullptr || en[0] == '\0'; }
};

// Language from a POSIX locale name ("it_IT.UTF-8", "de", "C"): English when
// the name is empty or not supported.
Lang parse_locale(const char* name);

// True for the languages whose glyphs the bundled IBM Plex fonts lacks.
bool needs_cjk_font(Lang lang);

// The user's language: ZEROTRACKER_LANG, then LC_ALL, LC_MESSAGES and LANG, the
// way the CardputerZero Store reads it. Read once, then cached.
Lang current();

// `text` in `lang` (or in current()), falling back to English.
const char* pick(const Text& text, Lang lang);
const char* pick(const Text& text);

// The setting value and back: EN IT FR DE ES JA ZH (English when unknown).
const char* code(Lang lang);
Lang from_code(const char* code);
// The language's own name (ENGLISH, ITALIANO, FRANCAIS...).
const char* native_name(Lang lang);
constexpr int kCount = 7;

} // namespace zt::i18n
