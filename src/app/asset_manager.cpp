/*
 * SPDX-FileCopyrightText: 2026 M5Stack Technology CO LTD
 *
 * SPDX-License-Identifier: MIT
 */

#include "asset_manager.h"

#include "logger.h"

#if LV_USE_FREETYPE
#include "src/libs/freetype/lv_freetype.h"
#endif

#include <algorithm>
#include <utility>

#ifndef APP_NAME
#define APP_NAME "template_app"
#endif

#ifndef APP_CMAKE_ASSETS_ROOT
#define APP_CMAKE_ASSETS_ROOT ""
#endif

#ifndef APP_SOURCE_ASSETS_ROOT
#define APP_SOURCE_ASSETS_ROOT "assets"
#endif

namespace app {
namespace {

#if !USE_DESKTOP
constexpr const char* kNotoSansCjkRegular =
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc";
constexpr const char* kNotoSansCjkBold =
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Medium.ttc";
#endif

bool path_exists(const std::filesystem::path& path) {
    std::error_code ec;
    return std::filesystem::exists(path, ec);
}

std::filesystem::path weakly_canonical_path(const std::filesystem::path& path) {
    std::error_code ec;
    auto canonical = std::filesystem::weakly_canonical(path, ec);
    return ec ? path.lexically_normal() : canonical;
}

#if !USE_DESKTOP
const lv_font_t* fallback_font(uint32_t size) {
    if (size <= 10) {
        return &lv_font_montserrat_10;
    }
    if (size <= 12) {
        return &lv_font_montserrat_12;
    }
    if (size <= 14) {
        return &lv_font_montserrat_14;
    }
    if (size <= 16) {
        return &lv_font_montserrat_16;
    }
    if (size <= 18) {
        return &lv_font_montserrat_18;
    }
    if (size <= 20) {
        return &lv_font_montserrat_20;
    }
    if (size <= 24) {
        return &lv_font_montserrat_24;
    }
    return &lv_font_montserrat_28;
}
#endif

} // namespace

struct AssetManager::LoadedFont {
    std::filesystem::path path;
    uint32_t size;
    lv_font_t* font;
};

AssetManager::AssetManager(std::string app_name, std::filesystem::path cmake_assets_root) {
    add_root_if_valid(std::move(cmake_assets_root));
    add_root_if_valid(default_cmake_assets_root());
    add_root_if_valid(APP_SOURCE_ASSETS_ROOT);
    add_root_if_valid(std::filesystem::path{"/usr/share"} / app_name);
    // Development runs: assets/ next to the executable (e.g. ~/zerotracker-dev).
    std::error_code ec;
    const auto exe = std::filesystem::read_symlink("/proc/self/exe", ec);
    if (!ec) {
        add_root_if_valid(exe.parent_path() / "assets");
    }
}

AssetManager::~AssetManager() {
#if LV_USE_FREETYPE
    for (auto& loaded_font : loaded_fonts_) {
        if (loaded_font && loaded_font->font) {
            lv_freetype_font_delete(loaded_font->font);
        }
    }

#endif
}

std::string AssetManager::default_app_name() {
    return APP_NAME;
}

std::filesystem::path AssetManager::default_cmake_assets_root() {
    return APP_CMAKE_ASSETS_ROOT;
}

const std::vector<std::filesystem::path>& AssetManager::roots() const {
    return roots_;
}

std::filesystem::path AssetManager::resolve(const std::filesystem::path& relative_path) const {
    if (relative_path.is_absolute() && path_exists(relative_path)) {
        return weakly_canonical_path(relative_path);
    }

    for (const auto& root : roots_) {
        auto candidate = root / relative_path;
        if (path_exists(candidate)) {
            return weakly_canonical_path(candidate);
        }
    }

    return {};
}

std::filesystem::path AssetManager::resolve_font(const std::filesystem::path& file_name) const {
    return resolve(std::filesystem::path{"fonts"} / file_name);
}

lv_font_t* AssetManager::load_font(const std::filesystem::path& file_name, uint32_t size) {
#if LV_USE_FREETYPE
    auto path = resolve_font(file_name);
    if (path.empty()) {
        LOG_WARN("font asset not found: {}", file_name.string());
        return nullptr;
    }

    auto existing = std::find_if(loaded_fonts_.begin(), loaded_fonts_.end(), [&](const auto& loaded_font) {
        return loaded_font && loaded_font->path == path && loaded_font->size == size;
    });
    if (existing != loaded_fonts_.end()) {
        return (*existing)->font;
    }

    lv_font_t* font = lv_freetype_font_create(path.string().c_str(),
                                              LV_FREETYPE_FONT_RENDER_MODE_BITMAP,
                                              size,
                                              LV_FREETYPE_FONT_STYLE_NORMAL);
    if (!font) {
        LOG_WARN("failed to load freetype font: {}", path.string());
        return nullptr;
    }

    loaded_fonts_.push_back(std::make_unique<LoadedFont>(LoadedFont{path, size, font}));
    LOG_INFO("loaded freetype font: {} ({})", path.string(), size);
    attach_cjk(font, size);
    return font;
#else
    LV_UNUSED(file_name);
    LV_UNUSED(size);
    LOG_WARN("FreeType font loading requested but LV_USE_FREETYPE is disabled");
    return nullptr;
#endif
}

lv_font_t* AssetManager::load_standard_font(uint32_t size, StandardFontWeight weight) {
#if USE_DESKTOP
    LV_UNUSED(size);
    LV_UNUSED(weight);
    return nullptr;
#else
    const char* path = weight == StandardFontWeight::Regular
                           ? kNotoSansCjkRegular
                           : kNotoSansCjkBold;
    auto* font = load_font(path, size);
    if (font) {
        font->fallback = fallback_font(size);
    }
    return font;
#endif
}

namespace {
// System fonts with the Japanese and Chinese glyphs IBM Plex lacks. LVGL opens
// face 0 of a collection, the Japanese one in Noto Sans CJK, so Chinese
// prefers Droid Sans Fallback. Debian paths first, then Arch for the desktop.
constexpr const char* kJapaneseFonts[] = {
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
};
constexpr const char* kChineseFonts[] = {
    "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
};
} // namespace

void AssetManager::set_cjk_fallback(const char* lang) {
    cjk_paths_.clear();
    const std::string l = lang ? lang : "";
    if (l == "ja") cjk_paths_.assign(std::begin(kJapaneseFonts), std::end(kJapaneseFonts));
    if (l == "zh") cjk_paths_.assign(std::begin(kChineseFonts), std::end(kChineseFonts));
    // The fonts already loaded take (or drop) the fallback too.
    // By index over the fonts loaded so far: attach_cjk loads more.
    const size_t n = loaded_fonts_.size();
    for (size_t i = 0; i < n; ++i) {
        LoadedFont* loaded = loaded_fonts_[i].get();
        if (!loaded || !loaded->font) continue;
        if (loaded->path.string().rfind("/usr/share/fonts/", 0) == 0) continue;  // a CJK font itself
        if (cjk_paths_.empty()) {
            loaded->font->fallback = nullptr;
        }
        else {
            attach_cjk(loaded->font, loaded->size);
        }
    }
}

void AssetManager::attach_cjk(lv_font_t* font, uint32_t size) {
    if (cjk_paths_.empty() || !font || font->fallback) return;
    const std::vector<const char*> paths = cjk_paths_;
    cjk_paths_.clear();  // the CJK font itself takes no fallback
    lv_font_t* cjk = nullptr;
    for (const char* path : paths) {
        if (!std::filesystem::exists(path)) continue;
        if ((cjk = load_font(path, size))) break;
    }
    cjk_paths_ = paths;
    if (cjk && cjk != font) font->fallback = cjk;
}

void AssetManager::add_root_if_valid(std::filesystem::path path) {
    if (path.empty()) {
        return;
    }

    path = weakly_canonical_path(path);
    if (!path_exists(path)) {
        return;
    }

    auto it = std::find(roots_.begin(), roots_.end(), path);
    if (it == roots_.end()) {
        roots_.push_back(std::move(path));
    }
}

} // namespace app
