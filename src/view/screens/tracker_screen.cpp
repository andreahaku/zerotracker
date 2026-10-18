/*
 * Tracker screen. See tracker_screen.h.
 */
#include "tracker_screen.h"

#include "help_text.h"
#include "audio_input.h"
#include "key_hints.h"
#include "i18n/i18n.h"

#include "asset_manager.h"
#include "audio_output.h"
#include "audio_volume.h"
#include "device_status.h"
#include "screenshot_service.h"
#include "tracker_keys.h"
#include "theme.h"

#include <algorithm>
#include <cstdio>
#include <cmath>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <tuple>

namespace screen {
namespace {

// Themes for the APP page. COLOR (once DARK) is the original palette.
constexpr std::array<const char*, 10> kThemeNames{"COLOR", "SYNTHWAVE", "SUNSET", "OCEAN", "AMBER",
                                                  "GREEN",  "DMG",       "ICE",    "CRIMSON", "MONO"};

// Font sizes offered on the APP page (px; rows are as tall as the size).
constexpr std::array<int, 7> kFontSizes{11, 12, 13, 14, 15, 16, 17};

// Bottom bar: the page-name tab's inset and vertical padding (px).
constexpr int kTabInset = 4;
constexpr int kTabPadV = 1;

constexpr const char* kPageNames[] = {"TIME", "MIX", "INST", "SONG", "APP", "OSC", "LIB"};
// The getting-started guide MANUAL points to.
constexpr const char* kManualUrl = "https://github.com/andreahaku/zerotracker/tree/main/docs/manual";
// LIBRARY's catalog; ZT_LIBRARY_URL overrides it (tests, a mirror).
constexpr const char* kLibraryUrl =
    "https://github.com/andreahaku/zerotracker/releases/download/library/packs.json";

// A pack or disk size: K, M or G, one decimal from 1 G.
std::string pack_size(uint64_t bytes) {
    char buf[16];
    if (bytes < 1000ull * 1000) std::snprintf(buf, sizeof buf, "%lluK", static_cast<unsigned long long>((bytes + 999) / 1000));
    else if (bytes < 1000ull * 1000 * 1000) std::snprintf(buf, sizeof buf, "%lluM", static_cast<unsigned long long>((bytes + 500000) / 1000000));
    else std::snprintf(buf, sizeof buf, "%.1fG", static_cast<double>(bytes) / 1e9);
    return buf;
}


// Songs folder: ZT_M8_ROOT, else ~/ZeroTracker (the device has no separate
// data card). The folder was ~/M8 before 15/10/2026: when only that one
// exists it is renamed, and `last_song` follows it.
std::string default_root(std::string& last_song) {
    if (const char* env = std::getenv("ZT_M8_ROOT"); env && *env) {
        return env;
    }
    namespace fs = std::filesystem;
    const char* home_env = std::getenv("HOME");
    const fs::path home = home_env ? home_env : ".";
    const fs::path root = home / "ZeroTracker", old = home / "M8";
    std::error_code ec;
    if (!fs::exists(root, ec) && fs::is_directory(old, ec)) {
        fs::rename(old, root, ec);
        if (ec) return old.string();  // keep working from the old folder
        const std::string prefix = old.string() + "/";
        if (last_song.rfind(prefix, 0) == 0) {
            last_song = root.string() + "/" + last_song.substr(prefix.size());
        }
    }
    return root.string();
}

std::string settings_path() {
    const char* xdg = std::getenv("XDG_CONFIG_HOME");
    const char* home = std::getenv("HOME");
    const std::string base = xdg && *xdg ? xdg : std::string(home ? home : ".") + "/.config";
    return base + "/zerotracker/settings.ini";
}

bool has_m8s_extension(const std::filesystem::path& p) {
    auto ext = p.extension().string();
    std::transform(ext.begin(), ext.end(), ext.begin(), ::tolower);
    return ext == ".m8s";
}

} // namespace

const TrackerScreen::Theme& TrackerScreen::theme() const {
    static const std::array<Theme, 10> themes{{
        {"COLOR", 0x000000, 0xD0D0D0, 0x2050A0, 0x203050, 0x808080, 0x40C060, 0x60C080, 0xFFFFFF,
         0xE04040, 0xE0C040, 0x606060,
         0xFFFFFF, 0x2860D0, 0xFFFFFF, 0xE03030, 0xFFFFFF, 0x1A1A1A, 0x181818, 0x000000,
         {0x72DF72, 0x1ECD72, 0x099A71, 0x2B7FA1, 0x525BAA},
         {0x40D060, 0xF0D030, 0xFF9020, 0xF03030}, true},
        // Multicolour palettes.
        {"SYNTHWAVE", 0x0D0221, 0xE0D0FF, 0x6A1B9A, 0x2A1050, 0x9080C0, 0xFF4FD8, 0x2DE2E6, 0xFFFFFF,
         0xFF3864, 0xF9C80E, 0x6050A0,
         0xFFFFFF, 0x7B2CBF, 0xFFFFFF, 0xFF2E97, 0xFFFFFF, 0x1A0B3A, 0x160A30, 0x0D0221,
         {0x2DE2E6, 0x3A86FF, 0x8338EC, 0xC23FD8, 0xFF2E97},
         {0x2DE2E6, 0xF9C80E, 0xFF8C42, 0xFF3864}, true},
        {"SUNSET", 0x120A0A, 0xF0E0D0, 0xA0402A, 0x3A1A18, 0xB08070, 0xFFB347, 0xFF8C42, 0xFFF4E0,
         0xD7263D, 0xFFD166, 0x705048,
         0xFFF4E0, 0xC0503A, 0xFFF4E0, 0xE63946, 0xFFFFFF, 0x22140F, 0x1C110D, 0x120A0A,
         {0xFFD166, 0xFF9F43, 0xFF6B6B, 0xD9467A, 0x8E3B8E},
         {0xFFD166, 0xFF9F43, 0xFF6B6B, 0xD7263D}, true},
        {"OCEAN", 0x03121C, 0xCFE8F0, 0x0E6B8C, 0x0A2A3C, 0x7FA8B8, 0x48CAE4, 0x90E0EF, 0xFFFFFF,
         0xEF476F, 0xFFD166, 0x40687A,
         0xFFFFFF, 0x0077B6, 0xFFFFFF, 0xEF476F, 0xFFFFFF, 0x08202E, 0x061A26, 0x03121C,
         {0xCAF0F8, 0x90E0EF, 0x48CAE4, 0x0096C7, 0x023E8A},
         {0x90E0EF, 0xFFD166, 0xF8961E, 0xEF476F}, true},
        // Monochrome palettes: every colour from one hue.
        {"AMBER", 0x0A0600, 0xE0A040, 0x805010, 0x402808, 0x906020, 0xF0B040, 0xFFC860, 0xFFF0C0,
         0x5A3810, 0xFFE080, 0x705020,
         0xFFF0C0, 0x805010, 0xFFF0C0, 0xFFC860, 0x0A0600, 0x1A1004, 0x160E04, 0x0A0600,
         {0xFFE0A0, 0xF0B040, 0xD09030, 0xA87020, 0x805010},
         {0xFFF0C0, 0xE0A040, 0xA87020, 0x705020}, false},
        {"GREEN", 0x000800, 0x50D070, 0x106030, 0x083018, 0x308040, 0x60F080, 0x90FFA0, 0xD0FFD0,
         0x185028, 0xB0FFC0, 0x206030,
         0xD0FFD0, 0x106030, 0xD0FFD0, 0x90FFA0, 0x000800, 0x041404, 0x061006, 0x000800,
         {0xC0FFD0, 0x60F080, 0x40C060, 0x309048, 0x207038},
         {0xD0FFD0, 0x60F080, 0x309048, 0x206030}, false},
        {"DMG", 0x0F380F, 0x8BAC0F, 0x306230, 0x1B4A1B, 0x6A8A10, 0x9BBC0F, 0x9BBC0F, 0xC8E060,
         0x1F4F1F, 0xC8E060, 0x306230,
         0xC8E060, 0x306230, 0xC8E060, 0x9BBC0F, 0x0F380F, 0x164416, 0x123C12, 0x0F380F,
         {0xC8E060, 0x9BBC0F, 0x8BAC0F, 0x6A8A10, 0x4A7020},
         {0xC8E060, 0x9BBC0F, 0x6A8A10, 0x306230}, false},
        {"ICE", 0x02060C, 0x9CC8F0, 0x1E4A78, 0x0C2238, 0x4A78A8, 0x7CB8F0, 0xB0D8FF, 0xE0F0FF,
         0x163050, 0xC8E4FF, 0x305070,
         0xE0F0FF, 0x1E4A78, 0xE0F0FF, 0xB0D8FF, 0x02060C, 0x081626, 0x06101C, 0x02060C,
         {0xD0E8FF, 0xA0CCF8, 0x70A8E0, 0x4A80B8, 0x2E5A88},
         {0xE0F0FF, 0x9CC8F0, 0x4A78A8, 0x305070}, false},
        {"CRIMSON", 0x0C0202, 0xF08070, 0x7A1A14, 0x3A0C0A, 0xA04038, 0xF06050, 0xFF9080, 0xFFE0D8,
         0x4A1410, 0xFFB8A8, 0x6A2820,
         0xFFE0D8, 0x7A1A14, 0xFFE0D8, 0xFF9080, 0x0C0202, 0x1C0806, 0x160604, 0x0C0202,
         {0xFFC0B0, 0xFF8C78, 0xE05848, 0xB03428, 0x802018},
         {0xFFE0D8, 0xF08070, 0xA04038, 0x6A2820}, false},
        {"MONO", 0x000000, 0xC8C8C8, 0x585858, 0x2C2C2C, 0x909090, 0xD0D0D0, 0xFFFFFF, 0xFFFFFF,
         0x484848, 0xE8E8E8, 0x606060,
         0xFFFFFF, 0x585858, 0xFFFFFF, 0xF0F0F0, 0x000000, 0x161616, 0x141414, 0x000000,
         {0xF0F0F0, 0xD0D0D0, 0xB0B0B0, 0x909090, 0x787878},
         {0xFFFFFF, 0xC0C0C0, 0x909090, 0x606060}, false},
    }};
    return themes[std::clamp(settings_.theme, 0, static_cast<int>(themes.size()) - 1)];
}

TrackerScreen::TrackerScreen(viewmodel::BaseViewModel& view_model, app::AssetManager& assets)
    : BaseScreen(view_model, assets) {
    init();
}

TrackerScreen::~TrackerScreen() {
    if (timer_) {
        lv_timer_delete(timer_);
    }
    zt_render_free(render_);  // cancels a running RENDER
    zt_library_free(library_);  // cancels a download
    // Quitting (ESC held) with unsaved edits: keep them in NAME.autosave.
    if (viewer_ && zt_viewer_dirty(viewer_)) {
        zt_viewer_autosave(viewer_);
    }
    zt_viewer_close(viewer_);
    zt_preview_destroy(preview_);
    // The audio thread must stop before the engine goes away.
    platform::audio::stop();
    zt_engine_destroy(engine_);
}

// REC.: START begins, STOP keeps the take as Samples/REC/RECnn.wav, sets it
// on the SAMPLER and opens it in the sample editor; CANCEL drops it.
void TrackerScreen::toggle_recording() {
    namespace ai = platform::audio_input;
    if (!ai::recording()) {
        rec_error_.clear();
        if (!ai::start(rec_error_)) {
            if (rec_error_.empty()) rec_error_ = "NO INPUT";
        }
        return;
    }
    const auto pcm = ai::stop();
    if (pcm.empty()) return;
    namespace fs = std::filesystem;
    std::error_code ec;
    const fs::path dir = fs::path(root_dir_) / "Samples" / "REC";
    fs::create_directories(dir, ec);
    fs::path path;
    for (int n = 0; n < 1000; ++n) {
        char name[16];
        std::snprintf(name, sizeof name, "REC%02d.wav", n);
        path = dir / name;
        if (!fs::exists(path, ec)) break;
    }
    if (!ai::write_wav(path.string(), pcm)) {
        rec_error_ = "WRITE FAILED";
        return;
    }
    close_recorder(true);
    if (viewer_ && zt_viewer_set_sample(viewer_, path.string().c_str())) {
        open_sample_editor(path.string());
    }
}

void TrackerScreen::close_recorder(bool keep) {
    if (!keep && platform::audio_input::recording()) platform::audio_input::stop();
    rec_open_ = false;
    rec_view_.set_visible(false);
}

bool TrackerScreen::draw_recorder() {
    rec_view_.set_visible(rec_open_);
    if (!rec_open_) return false;
    namespace ai = platform::audio_input;
    RecordView::State s;
    s.recording = ai::recording();
    s.seconds = ai::frames() / static_cast<double>(ai::kRate);
    s.level = ai::take_peak();
    s.peaks = ai::recent_peaks(320);
    s.title = tr({"REC. - MIC", "REC. - MICROFONO", "REC. - MICRO", "REC. - MIKROFON", "REC. - MICRÓFONO",
                  "録音 - マイク", "录音 - 麦克风"});
    s.status = s.recording ? tr({"RECORDING", "REGISTRA", "ENREGISTRE", "AUFNAHME", "GRABANDO", "録音中", "录音中"})
                           : tr({"READY", "PRONTO", "PRÊT", "BEREIT", "LISTO", "待機", "就绪"});
    s.hint = tr({"ENTER start/stop   ESC cancel", "ENTER avvia/ferma   ESC annulla",
                 "ENTER départ/arrêt   ESC annuler", "ENTER Start/Stopp   ESC abbrechen",
                 "ENTER iniciar/parar   ESC cancelar", "ENTER 開始/停止   ESC キャンセル", "ENTER 开始/停止   ESC 取消"});
    s.error = rec_error_;
    rec_view_.set(std::move(s));
    clear_grid();
    return true;
}

void TrackerScreen::close_key_hint() {
    if (hint_box_) lv_obj_delete(hint_box_);
    hint_box_ = nullptr;
}

// A small box over the page: "Key N" and what it does, in the APP language.
void TrackerScreen::show_key_hint(int key) {
    close_key_hint();
    const Theme& t = theme();
    const char* name = key == 4 ? "" : nav_slots()[key - 4].key;
    const char* what = key == 4 ? tr(hints::kPage)
                       : rec_open_ && std::strcmp(name, "STOP") == 0 ? tr(hints::kStopRecording)
                                                                     : tr(hints::for_key(name));
    char title[32];
    std::snprintf(title, sizeof title, tr(hints::kTitle), key);
    hint_box_ = lv_obj_create(lv_screen_active());
    lv_obj_remove_style_all(hint_box_);
    lv_obj_set_width(hint_box_, view::kScreenWidth - 24);
    lv_obj_set_height(hint_box_, LV_SIZE_CONTENT);
    lv_obj_set_style_bg_color(hint_box_, lv_color_hex(t.panel), 0);
    lv_obj_set_style_bg_opa(hint_box_, LV_OPA_COVER, 0);
    lv_obj_set_style_border_color(hint_box_, lv_color_hex(t.accent), 0);
    lv_obj_set_style_border_width(hint_box_, 2, 0);
    lv_obj_set_style_radius(hint_box_, 8, 0);
    lv_obj_set_style_pad_all(hint_box_, 8, 0);
    lv_obj_set_style_pad_row(hint_box_, 4, 0);
    lv_obj_set_flex_flow(hint_box_, LV_FLEX_FLOW_COLUMN);
    lv_obj_remove_flag(hint_box_, LV_OBJ_FLAG_SCROLLABLE);
    auto label = [&](const char* s, const lv_font_t* f, uint32_t color) {
        auto* l = lv_label_create(hint_box_);
        lv_label_set_text(l, s);
        lv_obj_set_width(l, lv_pct(100));
        lv_label_set_long_mode(l, LV_LABEL_LONG_WRAP);
        lv_obj_set_style_text_font(l, f, 0);
        lv_obj_set_style_text_color(l, lv_color_hex(color), 0);
    };
    label(title, bold_font_, t.accent);
    label(what, font_, t.fg);
    lv_obj_align(hint_box_, LV_ALIGN_CENTER, 0, 0);
    lv_obj_move_foreground(hint_box_);
    hint_shown_at_ = lv_tick_get();
}

// Japanese and Chinese take a system CJK font as the fallback of every font.
void TrackerScreen::apply_language() {
    const auto l = lang();
    assets().set_cjk_fallback(l == zt::i18n::Lang::Ja ? "ja" : l == zt::i18n::Lang::Zh ? "zh" : "");
}

void TrackerScreen::load_settings() {
    // Until APP sets one: the system's language, as the Store reads it.
    settings_.language = static_cast<int>(zt::i18n::current());
    std::ifstream in(settings_path());
    std::string line;
    while (std::getline(in, line)) {
        const auto eq = line.find('=');
        if (eq == std::string::npos) continue;
        const std::string key = line.substr(0, eq), value = line.substr(eq + 1);
        if (key == "font_size") {
            const int size = std::atoi(value.c_str());
            if (std::find(kFontSizes.begin(), kFontSizes.end(), size) != kFontSizes.end()) {
                settings_.font_size = size;
            }
        }
        else if (key == "spectrum") {
            settings_.spectrum = value != "off";
        }
        else if (key == "theme") {
            for (size_t i = 0; i < kThemeNames.size(); ++i) {
                if (value == kThemeNames[i]) settings_.theme = static_cast<int>(i);
            }
            if (value == "DARK") settings_.theme = 0;  // COLOR's old name
        }
        else if (key == "library_url") {
            settings_.library_url = value;
        }
        else if (key == "language") {
            settings_.language = static_cast<int>(zt::i18n::from_code(value.c_str()));
        }
        else if (key == "last_song") {
            settings_.last_song = value;
        }
    }
    apply_language();
}

void TrackerScreen::save_settings() const {
    const std::filesystem::path path = settings_path();
    std::error_code ec;
    std::filesystem::create_directories(path.parent_path(), ec);
    std::ofstream out(path);
    out << "font_size=" << settings_.font_size << "\n"
        << "theme=" << kThemeNames[settings_.theme] << "\n"
        << "spectrum=" << (settings_.spectrum ? "on" : "off") << "\n"
        << "language=" << zt::i18n::code(lang()) << "\n"
        << "last_song=" << settings_.last_song << "\n";
    if (!settings_.library_url.empty()) out << "library_url=" << settings_.library_url << "\n";
}

void TrackerScreen::build_content(lv_obj_t* content) {
    content_ = content;
    load_settings();
    build_grid();

    platform::tracker_keys::init();
    const std::string last_song = settings_.last_song;
    root_dir_ = default_root(settings_.last_song);
    if (settings_.last_song != last_song) save_settings();
    dir_ = root_dir_;
    copy_starter();
    list_directory();
    engine_ = zt_engine_create(44100, 0);
    if (engine_ && !platform::audio::start_engine(engine_)) {
        error_ = "no audio";
    }
    if (engine_) preview_ = zt_preview_create(engine_, root_dir_.c_str());
    // Development: LOAD for instruments or samples, until the INSTRUMENT
    // page opens it that way.
    if (const char* kind = std::getenv("ZT_BROWSE"); kind && *kind) {
        const std::string k = kind;
        if (k == "instrument") browse_for_ = BrowserView::Kind::Instrument;
        if (k == "sample") browse_for_ = BrowserView::Kind::Sample;
    }
    if (const char* open = std::getenv("ZT_OPEN"); open && *open) {
        open_song(open);
    }
    if (const char* keys = std::getenv("ZT_KEYS"); keys && *keys) {
        std::string all(keys), tok;
        for (char ch : all + ",") {
            if (ch == ',') {
                if (!tok.empty()) script_.push_back(tok);
                tok.clear();
            }
            else {
                tok += ch;
            }
        }
    }
    redraw();
    timer_ = lv_timer_create(tick_cb, 16, this);
}

void TrackerScreen::build_grid() {
    lv_obj_t* content = content_;
    lv_obj_clean(content);
    rows_.clear();
    overlays_.clear();
    shown_.clear();
    shown_bar_.clear();
    battery_checked_at_ = 0;
    const Theme& t = theme();
    set_meter_scale({t.colorful, t.chain[4], t.hi});

    lv_obj_set_style_bg_color(content, lv_color_hex(t.bg), 0);
    lv_obj_set_style_bg_opa(content, LV_OPA_COVER, 0);

    selection_ = lv_obj_create(content);
    lv_obj_remove_style_all(selection_);
    lv_obj_set_style_bg_color(selection_, lv_color_hex(t.selection), 0);
    lv_obj_set_style_bg_opa(selection_, LV_OPA_COVER, 0);
    lv_obj_add_flag(selection_, LV_OBJ_FLAG_HIDDEN);

    cursor_ = lv_obj_create(content);
    lv_obj_remove_style_all(cursor_);
    lv_obj_set_style_bg_color(cursor_, lv_color_hex(t.cursor), 0);
    lv_obj_set_style_bg_opa(cursor_, LV_OPA_COVER, 0);
    lv_obj_add_flag(cursor_, LV_OBJ_FLAG_HIDDEN);

    // IBM Plex Mono on rows as tall as the size (14 px by default; the APP
    // page changes the size). ZT_FONT=<file>
    // ZT_FONT_SIZE=<px> ZT_LINE_HEIGHT=<px> override it; the fallback is
    // LVGL's built-in unscii-8.
    font_ = &lv_font_unscii_8;
    bold_font_ = nullptr;
    const char* size_env = std::getenv("ZT_FONT_SIZE");
    const uint32_t size = size_env && *size_env ? static_cast<uint32_t>(std::atoi(size_env))
                                                : static_cast<uint32_t>(settings_.font_size);
    int line_height = static_cast<int>(size);
    // The APP font size applies to the whole UI: the
    // graphic pages' fonts, designed at 14 px, scale with it.
    ui_font_size_ = static_cast<int>(size);
    if (const char* file = std::getenv("ZT_FONT"); file && *file) {
#if LV_USE_FREETYPE
        if (auto* f = lv_freetype_font_create(file, LV_FREETYPE_FONT_RENDER_MODE_BITMAP, size,
                                              LV_FREETYPE_FONT_STYLE_NORMAL)) {
            font_ = f;
        }
#endif
        line_height = 0;
    }
    else if (auto* f = assets().load_font("IBMPlexMono-Regular.ttf", size)) {
        font_ = f;
        bold_font_ = assets().load_font("IBMPlexMono-Bold.ttf", size);
    }
    else {
        line_height = 0;
    }
    if (!bold_font_) {
        bold_font_ = font_;
    }
    cell_w_ = std::max<int>(1, lv_font_get_glyph_width(font_, '0', 0));
    cell_h_ = line_height > 0 ? line_height : std::max<int>(1, lv_font_get_line_height(font_));
    if (const char* lh = std::getenv("ZT_LINE_HEIGHT"); lh && *lh) {
        cell_h_ = std::max(1, std::atoi(lh));
    }
    // ZT_COLS spreads the glyphs over the full width with letter spacing.
    letter_space_ = 0;
    if (const char* c = std::getenv("ZT_COLS"); c && *c && std::atoi(c) > 0) {
        const int wanted = view::kScreenWidth / std::atoi(c);
        letter_space_ = std::max(0, wanted - cell_w_);
        cell_w_ = std::max(cell_w_, wanted);
    }
    cols_ = view::kScreenWidth / cell_w_;
    // Top bar one text line high, the page bar (with the spectrum) at the bottom.
    const int title_px = cell_h_;
    rows_count_ = std::max(1, (view::kScreenHeight - title_px - kNavHeight) / cell_h_);
    top_px_ = title_px;
    const int top = top_px_;
    const int label_h = std::max<int>(cell_h_, lv_font_get_line_height(font_));

    for (auto** bar : {&bar_left_, &bar_right_, &bar_bpm_}) {
        *bar = lv_label_create(content);
        lv_obj_set_style_text_font(*bar, font_, 0);
        lv_obj_set_style_text_color(*bar, lv_color_hex(t.bar_fg), 0);
        lv_label_set_text(*bar, "");
    }
    lv_obj_set_pos(bar_left_, 0, 0);
    // Song name in bold white.
    lv_obj_set_style_text_font(bar_left_, bold_font_, 0);
    lv_obj_set_style_text_color(bar_left_, lv_color_hex(t.hi), 0);
    lv_obj_align(bar_right_, LV_ALIGN_TOP_RIGHT, -2, 0);
    // The meters start at kScreenWidth - 90: the tempo ends 6 px before them.
    lv_obj_align(bar_bpm_, LV_ALIGN_TOP_RIGHT, -96, 0);
    shown_bpm_.clear();
    // 8 track level meters in the top band, left of the battery.
    {
        std::vector<int> x, w;
        for (int i = 0; i < 8; ++i) {
            x.push_back(i * 6);
            w.push_back(4);
        }
        meters_.create(content, view::kScreenWidth - 90, 1, 48, cell_h_ - 2, t.meter);
        meters_.set_layout(std::move(x), std::move(w));
    }
    // MIXER bars, shown on that page only; laid out from the meter area.
    mixer_bars_.create(content, 0, top_px_, view::kScreenWidth, 1, t.meter);
    mixer_bars_.set_visible(false);
    mixer_area_.fill(-1);

    for (int r = 0; r < rows_count_; ++r) {
        auto* label = lv_label_create(content);
        lv_obj_set_style_text_font(label, font_, 0);
        lv_obj_set_style_text_letter_space(label, letter_space_, 0);
        lv_obj_set_style_text_color(label, lv_color_hex(t.fg), 0);
        lv_label_set_long_mode(label, LV_LABEL_LONG_MODE_CLIP);
        // Full font height, so a pitch tighter than the font's line height
        // does not clip glyphs; rows may overlap in their empty margins.
        lv_obj_set_size(label, cols_ * cell_w_, label_h);
        lv_obj_set_pos(label, 0, top + r * cell_h_);
        lv_label_set_text(label, "");
        rows_.push_back(label);
    }

    // Highlight overlays: labels on an opaque background over the grid.
    overlays_.resize(96);
    for (auto& o : overlays_) {
        o.label = lv_label_create(content);
        lv_obj_set_style_text_letter_space(o.label, letter_space_, 0);
        lv_obj_set_style_bg_opa(o.label, LV_OPA_COVER, 0);
        lv_label_set_long_mode(o.label, LV_LABEL_LONG_MODE_CLIP);
        lv_obj_add_flag(o.label, LV_OBJ_FLAG_HIDDEN);
    }

    // TIMELINE as graphics, between the top bar and the bottom bar.
    const int bottom_y = view::kScreenHeight - kNavHeight;
    const lv_font_t* badge_font = assets().load_font("IBMPlexMono-Bold.ttf", ui_px(11));
    timeline_.create(content, 0, top_px_, view::kScreenWidth, bottom_y - top_px_, font_,
                     badge_font ? badge_font : bold_font_,
                     {t.bg, t.cursor, t.selection, t.panel, t.badge, t.on_badge, t.dim,
                      t.solo, t.alert, t.on_alert, t.hi, t.chain});
    timeline_.set_visible(false);
    {
        // The grid's fonts: the APP font size applies here too.
        chain_phrase_.create(content, 0, top_px_, view::kScreenWidth, bottom_y - top_px_,
                             font_, bold_font_, assets().load_font("IBMPlexMono-Bold.ttf", ui_px(11)),
                             {t.bg, t.panel, t.fg, t.dim, t.hi, t.accent, t.cursor, t.hi, t.selection,
                              t.alert, t.on_alert, t.accent, t.solo, t.chain});
        chain_phrase_.set_visible(false);
        instrument_.create(content, 0, top_px_, view::kScreenWidth, bottom_y - top_px_, font_,
                           bold_font_, cell_w_, cell_h_,
                           {t.bg, t.panel, t.fg, t.dim, t.hi, t.accent, t.cursor, t.hi, t.selection,
                            t.chain});
        instrument_.set_visible(false);
        const lv_font_t* small = assets().load_font("IBMPlexMono-Regular.ttf", ui_px(11));
        sample_view_.create(content, 0, top_px_, view::kScreenWidth, bottom_y - top_px_, font_,
                            bold_font_, small,
                            {t.bg, t.panel, t.fg, t.dim, t.hi, t.accent, t.selection, t.solo, t.alert,
                             t.playing, t.chain[1]});
        sample_view_.set_visible(false);
        eq_view_.create(content, 0, top_px_, view::kScreenWidth, bottom_y - top_px_, font_,
                        bold_font_, small,
                        {t.bg, t.panel, t.fg, t.dim, t.hi, t.accent,
                         {t.chain[0], t.chain[2], t.chain[4]}});
        eq_view_.set_visible(false);
    }
    const lv_font_t* small_font = badge_font ? badge_font : bold_font_;
    mixer_view_.create(content, 0, top_px_, view::kScreenWidth, bottom_y - top_px_, font_, small_font,
                       {t.badge, t.alert, t.fg, t.dim, t.unlit, t.alert, t.solo, t.solo,
                        t.on_badge, t.on_alert, t.on_solo, t.hi});
    mixer_view_.set_visible(false);
    scopes_.create(content, 0, top_px_, view::kScreenWidth, bottom_y - top_px_, t.bg);
    scopes_.set_visible(false);

    // HOME and LOAD cover everything above the bottom bar, top bar included.
    {
        auto font = [this](const char* file, int px, const lv_font_t* fallback) {
            const lv_font_t* f = assets().load_font(file, px);
            return f ? f : fallback;
        };
        const lv_font_t* small = font("IBMPlexMono-Regular.ttf", ui_px(11), font_);
        const lv_font_t* label = font("IBMPlexMono-Bold.ttf", ui_px(14), bold_font_);
        song_view_.create(content, 0, top_px_, view::kScreenWidth, bottom_y - top_px_,
                          {font("IBMPlexMono-Bold.ttf", ui_px(30), bold_font_), label, small,
                           font("Phosphor-Fill.ttf", ui_px(16), font_)},
                          {t.bg, t.panel, t.fg, t.dim, t.hi, t.accent, t.cursor, t.hi, t.alert});
        song_view_.set_visible(false);
        app_view_.create(content, 0, top_px_, view::kScreenWidth, bottom_y - top_px_,
                         {label, small, font("Phosphor-Fill.ttf", ui_px(16), font_)},
                         {t.bg, t.panel, t.fg, t.dim, t.hi, t.accent, t.cursor, t.hi});
        app_view_.set_visible(false);
        tracks_view_.create(content, 0, top_px_, view::kScreenWidth, bottom_y - top_px_,
                            {label, small, font("Phosphor-Fill.ttf", ui_px(14), font_)},
                            {t.bg, t.panel, t.fg, t.dim, t.hi, t.accent, t.cursor, t.hi, t.alert, t.badge,
                             t.on_badge, t.muted, t.solo, t.on_solo, t.playing, t.chain});
        tracks_view_.set_visible(false);
        rec_view_.create(content, 0, top_px_, view::kScreenWidth, bottom_y - top_px_,
                         {font("IBMPlexMono-Bold.ttf", ui_px(30), bold_font_), label, small,
                          font("Phosphor-Fill.ttf", ui_px(18), font_)},
                         {t.bg, t.panel, t.fg, t.dim, t.hi, t.accent, t.alert});
        rec_view_.set_visible(false);
        fx_picker_view_.create(content, 0, top_px_, view::kScreenWidth, bottom_y - top_px_, {label, small},
                               {t.bg, t.panel, t.fg, t.dim, t.hi, t.accent, t.cursor, t.hi});
        fx_picker_view_.set_visible(false);
        home_.create(content, 0, 0, view::kScreenWidth, bottom_y,
                     {font("IBMPlexMono-Bold.ttf", ui_px(22), bold_font_), label, small,
                      font("Phosphor-Fill.ttf", ui_px(16), font_)},
                     {t.bg, t.panel, t.fg, t.dim, t.hi, t.accent, t.cursor, t.hi, t.chain},
                     "v0.1.0");
        home_.set_visible(false);
        browser_.create(content, 0, 0, view::kScreenWidth, bottom_y,
                        {font("IBMPlexMono-Bold.ttf", ui_px(13), bold_font_),
                         font("IBMPlexMono-Regular.ttf", ui_px(14), font_), small,
                         font("Phosphor-Fill.ttf", ui_px(14), font_)},
                        {t.bg, t.panel, t.fg, t.dim, t.hi, t.accent, t.cursor, t.hi, t.chain[2],
                         t.chain[0], t.chain[3], t.chain[4], t.dim});
        browser_.set_visible(false);
        library_view_.create(content, 0, 0, view::kScreenWidth, bottom_y,
                             {font("IBMPlexMono-Bold.ttf", ui_px(13), bold_font_),
                              font("IBMPlexMono-Regular.ttf", ui_px(14), font_), small,
                              font("Phosphor-Fill.ttf", ui_px(14), font_)},
                             {t.bg, t.panel, t.fg, t.dim, t.hi, t.accent, t.cursor, t.hi, t.chain[2],
                              t.chain[0], t.chain[3], t.chain[4], t.dim});
        library_view_.set_visible(false);
        library_view_.set_two_line(true);
        library_view_.set_note(tr({"Download songs, instruments, samples", "Scarica canzoni, strumenti e sample", "Morceaux, instruments, samples", "Songs, Instrumente, Samples laden", "Descarga canciones, sonidos, samples", "ソング・音色・サンプルを入手", "下载歌曲、乐器和采样"}));
        library_view_.set_words(tr({"packs", "pacchetti", "packs", "Pakete", "paquetes", "パック", "资源包"}),
                                tr({"no packs yet", "nessun pacchetto", "aucun pack", "keine Pakete", "sin paquetes", "パックなし", "暂无资源包"}));
        home_.set_tagline(tr({"pocket music tracker", "tracker musicale tascabile", "tracker musical de poche", "Taschen-Musiktracker", "tracker musical de bolsillo", "ポケット音楽トラッカー", "口袋音乐音序器"}));
        browser_.set_words(tr({"items", "elementi", "éléments", "Einträge", "elementos", "項目", "项"}), tr({"nothing here", "cartella vuota", "dossier vide", "leerer Ordner", "carpeta vacía", "空のフォルダ", "空文件夹"}));
    }

    // Bottom bar: the spectrum of the output is its animated background
    //, the labels go over it in white.
    nav_bg_ = lv_obj_create(content);
    lv_obj_remove_style_all(nav_bg_);
    lv_obj_set_style_bg_color(nav_bg_, lv_color_hex(t.bg), 0);
    lv_obj_set_style_bg_opa(nav_bg_, LV_OPA_COVER, 0);
    lv_obj_set_size(nav_bg_, view::kScreenWidth, kNavHeight);
    lv_obj_set_pos(nav_bg_, 0, bottom_y);
    // The spectrum fills the bar right of the page-name tab, which is opaque.
    const int slot_w = view::kScreenWidth / static_cast<int>(nav_labels_.size());
    const int spectrum_x = slot_w;
    const int spectrum_w = view::kScreenWidth - spectrum_x;
    constexpr int kBands = 32;
    if (!spectrum_) {
        spectrum_ = std::make_unique<Spectrum>(1024, kBands, 44100.0f, 40.0f, 16000.0f);
        tap_.resize(spectrum_->size());
    }
    {
        const int band_w = spectrum_w / kBands;
        std::vector<int> x, w;
        for (int b = 0; b < kBands; ++b) {
            x.push_back(b * band_w);
            w.push_back(band_w - 1);
        }
        spectrum_bars_.create(content, spectrum_x, bottom_y, spectrum_w, kNavHeight, t.selection);
        spectrum_bars_.set_layout(std::move(x), std::move(w));
    }
    icon_font_ = assets().load_font("Phosphor-Fill.ttf", ui_px(18));
    // At most 16 px: six characters still fit a fifth of the bar.
    nav_font_ = assets().load_font("IBMPlexMono-Bold.ttf", std::min(ui_px(15), 16));
    if (!nav_font_) nav_font_ = bold_font_;
    for (size_t i = 0; i < nav_labels_.size(); ++i) {
        auto* l = lv_label_create(content);
        lv_obj_set_width(l, slot_w);
        lv_obj_set_style_text_align(l, LV_TEXT_ALIGN_CENTER, 0);
        lv_label_set_long_mode(l, LV_LABEL_LONG_MODE_CLIP);
        lv_obj_set_pos(l, static_cast<int>(i) * slot_w, bottom_y);
        if (i == 0) {
            // The page-name tab, inset from the screen edge and the next slot.
            lv_obj_set_width(l, slot_w - 2 * kTabInset);
            lv_obj_set_x(l, kTabInset);
            lv_obj_set_style_radius(l, 4, 0);
            lv_obj_set_style_pad_ver(l, kTabPadV, 0);
        }
        lv_label_set_text(l, "");
        nav_labels_[i] = l;
        nav_shown_[i] = "\x01";  // force the first update
    }

    // The question modal, above everything, hidden until needed.
    modal_ = lv_obj_create(content);
    lv_obj_remove_style_all(modal_);
    // The keys line at most 12 px: it holds three keys and their actions.
    const lv_font_t* small = assets().load_font("IBMPlexMono-Regular.ttf", std::min(ui_px(11), 12));
    if (!small) small = font_;
    // Title, body and keys one line each, at the APP font size.
    const int title_h = lv_font_get_line_height(nav_font_), body_h = lv_font_get_line_height(font_),
              keys_h = lv_font_get_line_height(small);
    // Room for RENDER's progress bar between the name and the keys.
    modal_base_h_ = std::max(86, 16 + 4 + title_h + body_h + keys_h + 12 + 10);
    modal_keys_font_ = small;
    lv_obj_set_size(modal_, 296, modal_base_h_);
    lv_obj_align(modal_, LV_ALIGN_CENTER, 0, -8);
    lv_obj_set_style_bg_color(modal_, lv_color_hex(t.bg), 0);
    lv_obj_set_style_bg_opa(modal_, LV_OPA_COVER, 0);
    lv_obj_set_style_border_color(modal_, lv_color_hex(t.alert), 0);
    lv_obj_set_style_border_width(modal_, 2, 0);
    lv_obj_set_style_radius(modal_, 10, 0);
    lv_obj_set_style_pad_all(modal_, 8, 0);
    lv_obj_remove_flag(modal_, LV_OBJ_FLAG_SCROLLABLE);
    auto modal_label = [&](const lv_font_t* f, uint32_t color, lv_align_t align, int y) {
        auto* l = lv_label_create(modal_);
        lv_obj_set_size(l, 276, lv_font_get_line_height(f));  // one line: DOTS clips it
        lv_obj_set_style_text_font(l, f, 0);
        lv_obj_set_style_text_color(l, lv_color_hex(color), 0);
        lv_obj_set_style_text_align(l, LV_TEXT_ALIGN_CENTER, 0);
        lv_label_set_long_mode(l, LV_LABEL_LONG_MODE_DOTS);
        lv_obj_align(l, align, 0, y);
        return l;
    };
    modal_title_ = modal_label(nav_font_, t.hi, LV_ALIGN_TOP_MID, 0);
    modal_body_ = modal_label(font_, t.fg, LV_ALIGN_TOP_MID, title_h + 4);
    modal_keys_ = modal_label(small, t.accent, LV_ALIGN_BOTTOM_MID, 0);
    // RENDER's progress, under the name.
    modal_bar_ = lv_bar_create(modal_);
    lv_obj_set_size(modal_bar_, 260, 6);
    lv_obj_align(modal_bar_, LV_ALIGN_TOP_MID, 0, title_h + 4 + body_h + 4);
    lv_bar_set_range(modal_bar_, 0, 1000);
    lv_obj_set_style_bg_color(modal_bar_, lv_color_hex(t.panel), LV_PART_MAIN);
    lv_obj_set_style_bg_opa(modal_bar_, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_bg_color(modal_bar_, lv_color_hex(t.accent), LV_PART_INDICATOR);
    lv_obj_set_style_radius(modal_bar_, 3, LV_PART_MAIN);
    lv_obj_set_style_radius(modal_bar_, 3, LV_PART_INDICATOR);
    lv_obj_add_flag(modal_bar_, LV_OBJ_FLAG_HIDDEN);
    lv_obj_add_flag(modal_, LV_OBJ_FLAG_HIDDEN);
    modal_shown_.clear();

    // MANUAL: QR code left, title, address and hint right; black on white
    // so phones read the code under any theme.
    manual_panel_ = lv_obj_create(content);
    lv_obj_remove_style_all(manual_panel_);
    lv_obj_set_size(manual_panel_, view::kScreenWidth, view::kScreenHeight - kNavHeight);
    lv_obj_set_style_bg_color(manual_panel_, lv_color_hex(t.bg), 0);
    lv_obj_set_style_bg_opa(manual_panel_, LV_OPA_COVER, 0);
    lv_obj_remove_flag(manual_panel_, LV_OBJ_FLAG_SCROLLABLE);
    auto* qr = lv_qrcode_create(manual_panel_);
    lv_qrcode_set_size(qr, 118);
    lv_qrcode_set_dark_color(qr, lv_color_black());
    lv_qrcode_set_light_color(qr, lv_color_white());
    lv_qrcode_update(qr, kManualUrl, static_cast<uint32_t>(std::strlen(kManualUrl)));
    lv_obj_set_style_border_color(qr, lv_color_white(), 0);
    lv_obj_set_style_border_width(qr, 4, 0);
    lv_obj_set_pos(qr, 8, 8);
    auto manual_label = [&](const lv_font_t* f, uint32_t color, int y, const char* text) {
        auto* l = lv_label_create(manual_panel_);
        lv_obj_set_width(l, view::kScreenWidth - 146);
        lv_obj_set_style_text_font(l, f, 0);
        lv_obj_set_style_text_color(l, lv_color_hex(color), 0);
        lv_label_set_long_mode(l, LV_LABEL_LONG_MODE_WRAP);
        lv_label_set_text(l, text);
        lv_obj_set_pos(l, 140, y);
        return l;
    };
    manual_label(nav_font_, t.hi, 10, tr({"Getting started", "Per iniziare", "Pour commencer", "Erste Schritte", "Primeros pasos", "はじめに", "入门"}));
    manual_label(small, t.fg, 34, tr({"Scan the code for the guide to ZeroTracker and links to learn the M8 workflow.", "Inquadra il codice: la guida a ZeroTracker e i link per imparare il flusso dell'M8.", "Scannez le code : le guide de ZeroTracker et des liens pour apprendre le M8.", "Code scannen: Anleitung zu ZeroTracker und Links zum M8-Workflow.", "Escanea el código: la guía de ZeroTracker y enlaces para aprender el M8.", "コードを読み取ると、ZeroTrackerのガイドとM8の学習リンクが開きます。", "扫码查看 ZeroTracker 指南和学习 M8 的链接。"}));
    manual_label(small, t.accent, 100, "github.com/andreahaku\n/zerotracker");
    lv_obj_add_flag(manual_panel_, LV_OBJ_FLAG_HIDDEN);
}

void TrackerScreen::tick_cb(lv_timer_t* timer) {
    static_cast<TrackerScreen*>(lv_timer_get_user_data(timer))->tick();
}

void TrackerScreen::next_page() {
    if (!viewer_) {
        // Without a song: HOME (or the browser), APP, LIBRARY.
        page_ = page_ == 0 ? kPageApp : page_ == kPageApp ? kPageLibrary : 0;
    }
    else {
        // On OSCIL key 4 (its tab reads MIX) goes back to MIX, held or not.
        page_ = page_ == kPageOscil ? ZT_PAGE_MIXER
                : page_ == kPageApp ? kPageLibrary
                : page_ == kPageLibrary ? 0
                                        : page_ + 1;
        if (page_ < kPageApp) {
            zt_viewer_set_page(viewer_, page_);
        }
    }
    show_page_library();
    confirm_close_ = false;
    redraw();
}

// Key 4 held: the page before, the reverse of next_page().
void TrackerScreen::previous_page() {
    if (!viewer_) {
        page_ = page_ == 0 ? kPageLibrary : page_ == kPageLibrary ? kPageApp : 0;
    }
    else {
        // OSCIL and TRK both go back to MIX.
        if (page_ == kPageOscil || page_ == ZT_PAGE_MIXER + 1) {
            page_ = ZT_PAGE_MIXER;
        }
        else {
            page_ = page_ == 0 ? kPageLibrary : page_ == kPageLibrary ? kPageApp : page_ - 1;
        }
        if (page_ < kPageApp) {
            zt_viewer_set_page(viewer_, page_);
        }
    }
    show_page_library();
    confirm_close_ = false;
    redraw();
}

// LIBRARY opens when its page comes up and closes when key 4 leaves it.
void TrackerScreen::show_page_library() {
    if (page_ == kPageLibrary && !library_open_) open_library();
    else if (page_ != kPageLibrary && library_open_) close_library();
}

// ZT_KEYS: comma-separated steps, one per tick group. A step is buttons joined
// by '+': UP DOWN LEFT RIGHT SHIFT PLAY OPTION EDIT, e.g. "DOWN,SHIFT+RIGHT";
// or PAGE (key 4), PAGEBACK (key 4 held), ENTER / ESC / DEL, TYPE<chars> (text entry), SOFT1-SOFT4 (keys 5-8 off page 1), SHOT (screenshot to ~/Pictures/Screenshots), WAIT<ms>, or QUIT.
void TrackerScreen::run_script_step() {
    const std::string& step = script_[script_pos_];
    // WAIT<ms> holds the script; QUIT closes the app (timed device tests).
    if (step.rfind("WAIT", 0) == 0) {
        const uint32_t now = lv_tick_get();
        if (script_wait_until_ == 0) {
            script_wait_until_ = now + static_cast<uint32_t>(std::atoi(step.c_str() + 4));
        }
        if (now < script_wait_until_) {
            return;
        }
        script_wait_until_ = 0;
        ++script_pos_;
        return;
    }
    ++script_pos_;
    if (step == "QUIT") {
        view_model().request_quit();
        return;
    }
    if (step == "PAGE") {
        next_page();
        return;
    }
    if (step == "PAGEBACK") {
        previous_page();
        return;
    }
    // The RENDER modal takes the scripted keys too.
    if (render_modal_ || render_choice_ >= 0) {
        const uint32_t b = step == "LEFT" ? ZT_BTN_LEFT : step == "RIGHT" ? ZT_BTN_RIGHT : 0;
        if (b || step == "ENTER" || step == "ESC") {
            render_modal_key(b, step == "ENTER", step == "ESC", 0, true);
            return;
        }
    }
    if (manual_open_ && step != "SHOT" && step.rfind("WAIT", 0) != 0) {
        manual_open_ = false;
        redraw();
        return;
    }
    // LIBRARY takes the scripted keys too.
    if (library_open_ && step != "SHOT" && step.rfind("WAIT", 0) != 0) {
        const uint32_t b = step == "UP" ? ZT_BTN_UP : step == "DOWN" ? ZT_BTN_DOWN : 0;
        const uint32_t soft = step.size() == 5 && step.rfind("SOFT", 0) == 0 ? 1u << (step[4] - '1') : 0;
        library_input(b, step == "ENTER", step == "ESC", soft, true);
        redraw();
        return;
    }
    if (step == "RECORD") {  // as REC. on an empty SAMPLE
        rec_open_ = true;
        rec_error_.clear();
        redraw();
        return;
    }
    if (rec_open_ && (step == "ENTER" || step == "ESC")) {
        if (step == "ENTER") toggle_recording();
        else close_recorder(false);
        redraw();
        return;
    }
    // The sample editor takes the script's keys too.
    if ((sample_edit_ || eq_open_) && step != "SHOT" && step.rfind("WAIT", 0) != 0) {
        uint32_t buttons = 0, soft = 0;
        bool enter = false, escape = false, del = false;
        std::string typed;
        if (step.rfind("TYPE", 0) == 0) typed = step.substr(4);
        else if (step == "ENTER") enter = true;
        else if (step == "ESC") escape = true;
        else if (step == "DEL") del = true;
        else if (step.size() == 5 && step.rfind("SOFT", 0) == 0) soft = 1u << (step[4] - '1');
        else {
            size_t start = 0;
            while (start <= step.size()) {
                const size_t end = std::min(step.find('+', start), step.size());
                const std::string b = step.substr(start, end - start);
                if (b == "UP") buttons |= ZT_BTN_UP;
                else if (b == "DOWN") buttons |= ZT_BTN_DOWN;
                else if (b == "LEFT") buttons |= ZT_BTN_LEFT;
                else if (b == "RIGHT") buttons |= ZT_BTN_RIGHT;
                else if (b == "EDIT") buttons |= ZT_BTN_EDIT;
                else if (b == "PLAY") buttons |= ZT_BTN_PLAY;
                start = end + 1;
            }
        }
        const bool changed = eq_open_
            ? eq_editor_input(buttons & ~ZT_BTN_EDIT, buttons, enter, escape, del, soft, typed, false)
            : sample_editor_input(buttons & ~ZT_BTN_EDIT, buttons, enter, escape, del, soft, typed, false);
        if (changed) redraw();
        return;
    }
    // Text entry: TYPE<chars> appends, ENTER writes the NAME.
    if (text_mode_ && step.rfind("TYPE", 0) == 0) {
        text_ = (text_ + step.substr(4)).substr(0, text_max_);
        redraw();
        return;
    }
    if (text_mode_ && step == "ENTER") {
        zt_viewer_set_text(viewer_, text_.c_str());
        text_mode_ = false;
        redraw();
        return;
    }
    if (step == "EDIT" && open_text_entry()) {
        redraw();
        return;
    }
    // The new keys (ENTER / ESC / DEL) on a song view or in LOAD.
    if (step == "ENTER" || step == "ESC" || step == "DEL") {
        if (browsing_ && step == "ENTER") open_entry();
        else if (browsing_ && step == "ESC") end_browse();
        else if (viewer_ && page_ == ZT_PAGE_TIMELINE) {
            if (step == "ENTER") {
                if (zt_viewer_ux_enter(viewer_) == 2) open_text_entry();
                after_input();
            }
            else if (step == "ESC") {
                if (!zt_viewer_ux_back(viewer_)) {
                    if (zt_viewer_dirty(viewer_)) confirm_close_ = true;
                    else close_song();
                }
            }
            else zt_viewer_ux_clear(viewer_);
        }
        else if (viewer_ && page_ == ZT_PAGE_TRACKS) {
            if (step == "ENTER") {
                zt_viewer_tracks_enter(viewer_);
                page_ = zt_viewer_page(viewer_);
            }
            else if (step == "ESC") {
                if (zt_viewer_ux_state(viewer_) & ZT_UX_EDITING) zt_viewer_ux_back(viewer_);
                else {
                    page_ = 0;
                    zt_viewer_set_page(viewer_, 0);
                }
            }
        }
        redraw();
        return;
    }
    if (step.size() == 5 && step.rfind("SOFT", 0) == 0 && step[4] >= '1' && step[4] <= '4') {
        soft_key(step[4] - '0');
        redraw();
        return;
    }
    if (step.size() == 5 && step.rfind("HINT", 0) == 0 && step[4] >= '4' && step[4] <= '8') {
        show_key_hint(step[4] - '0');  // SYM+4..8
        return;
    }
    if (step == "SHOT") {
        redraw();
        lv_refr_now(nullptr);
        const auto shot = platform::screenshot::capture_active_screen();
        std::fprintf(stderr, "[shot] %s\n", shot.success ? shot.path.c_str() : shot.error.c_str());
        return;
    }
    if (instrument_preview_) {  // as a key press: ends the instrument preview
        end_instrument_preview();
        redraw();
        return;
    }
    uint32_t held = 0;
    size_t start = 0;
    while (start <= step.size()) {
        const size_t end = std::min(step.find('+', start), step.size());
        const std::string b = step.substr(start, end - start);
        if (b == "UP") held |= ZT_BTN_UP;
        else if (b == "DOWN") held |= ZT_BTN_DOWN;
        else if (b == "LEFT") held |= ZT_BTN_LEFT;
        else if (b == "RIGHT") held |= ZT_BTN_RIGHT;
        else if (b == "SHIFT") held |= ZT_BTN_SHIFT;
        else if (b == "PLAY") held |= ZT_BTN_PLAY;
        else if (b == "OPTION") held |= ZT_BTN_OPTION;
        else if (b == "EDIT") held |= ZT_BTN_EDIT;
        start = end + 1;
    }
    // Modifiers count as held, the other buttons as newly pressed.
    const uint32_t modifiers = ZT_BTN_SHIFT | ZT_BTN_OPTION | ZT_BTN_EDIT;
    const uint32_t pressed = (held & ~modifiers) ? (held & ~modifiers) : held;
    if (page_ == kPageApp) {
        handle_app(held, pressed);
    }
    else if (browsing_) {
        handle_browser(pressed);
    }
    else if (viewer_) {
        zt_viewer_input(viewer_, held, pressed);
        if (page_ == ZT_PAGE_TRACKS) page_ = zt_viewer_page(viewer_);  // SHIFT+RIGHT opens the instrument
        after_input();
    }
    else {
        handle_home(pressed);
    }
    redraw();
}

namespace {
bool g_exit_blocked = false;
}

bool exit_blocked() {
    return g_exit_blocked;
}

void TrackerScreen::tick() {
    // ESC held quits only with nothing to cancel (an edit, a selection,
    // text entry, LOAD, the SAVE? question).
    g_exit_blocked = text_mode_ || browsing_ || library_open_ || confirm_close_ || sample_edit_ || eq_open_ ||
                     (viewer_ && (zt_viewer_ux_state(viewer_) &
                                  (ZT_UX_EDITING | ZT_UX_SELECTING | ZT_UX_PICKER)));
    update_render();
    if (library_open_ && library_) {
        // The catalog arriving and a job's progress redraw the list or the modal.
        uint32_t permille = 0;
        const uint8_t state = zt_library_status(library_, &permille, nullptr, 0);
        const uint32_t now = lv_tick_get();
        const bool busy = state == ZT_LIB_DOWNLOADING || state == ZT_LIB_INSTALLING || state == ZT_LIB_FETCHING;
        if (state != lib_shown_state_ || (busy && now - last_animation_ >= 200)) {
            lib_shown_state_ = state;
            last_animation_ = now;
            redraw();
        }
    }
    if (viewer_) {
        zt_viewer_tick(viewer_);
        // Playhead and meters move without key presses: redraw at ~20 Hz.
        const uint32_t now = lv_tick_get();
        // Not under the sample editor: it redraws only on a change. The EQ
        // editor's spectrum moves while the song or the instrument plays
        // (draw_eq_editor keeps it to about 16 frames a second).
        const bool eq_live = eq_open_ && (zt_viewer_playing(viewer_) || instrument_preview_);
        if (!sample_edit_ && (!eq_open_ || eq_live) && now - last_redraw_ >= 50) {
            last_redraw_ = now;
            redraw();
        }
    }
    if (!viewer_ && page_ == 0 && !browsing_) {
        // The logo's bars at ~30 fps; only the logo is redrawn.
        const uint32_t now = lv_tick_get();
        if (now - last_animation_ >= 33) {
            last_animation_ = now;
            home_.animate(now);
        }
    }
    if (instrument_preview_ && !zt_preview_playing(preview_)) end_instrument_preview();
    if ((!viewer_ || browsing_ || instrument_preview_) && preview_) {
        zt_preview_tick(preview_);
        // PLAY/STOP follows the preview, which also ends by itself.
        const bool playing = zt_preview_playing(preview_);
        if (playing != preview_shown_playing_) {
            preview_shown_playing_ = playing;
            redraw();
        }
    }
    if (sample_edit_) {
        // While a preview plays only the playhead moves (no waveform redraw,
        // which would take the CPU from the audio); a redraw at the end.
        const uint32_t now = lv_tick_get();
        if (se_playing_ && now - last_animation_ >= 33) {
            last_animation_ = now;
            ZtSampleInfo info{};
            zt_sample_info(sample_edit_, &info);
            const uint64_t heard = uint64_t(now - se_play_tick_) * info.rate / 1000;
            if (se_play_from_ + heard >= se_play_to_) {
                se_playing_ = false;
                sample_view_.set_playhead(-1);
                redraw();
            }
            else {
                sample_view_.set_playhead(se_play_from_ + static_cast<int64_t>(heard));
            }
        }
        if (!se_message_.empty() && now - se_message_at_ >= 2500) {
            se_message_.clear();
            redraw();
        }
    }
    if (!browser_message_.empty() && lv_tick_get() - browser_message_at_ >= 2500) {
        browser_message_.clear();
        redraw();
    }
    if (script_pos_ < script_.size()) {
        run_script_step();
        return;
    }
    auto state = platform::tracker_keys::poll();
    // Keys 5-8 are the page's buttons, named in the bottom bar. On page 1
    // with a song, key 5 is EDIT: held with the arrows it changes the value,
    // tapped alone it toggles edit mode (on release, if no arrow was used).
    const bool on_song_view =
        viewer_ && page_ == ZT_PAGE_TIMELINE && !browsing_ && !text_mode_ && !sample_edit_ &&
        !eq_open_;
    uint32_t soft_pressed = state.soft & ~last_soft_;
    const uint32_t soft_released = ~state.soft & last_soft_;
    last_soft_ = state.soft;
    const uint32_t arrows = ZT_BTN_UP | ZT_BTN_DOWN | ZT_BTN_LEFT | ZT_BTN_RIGHT;
    // Ctrl+letter is a shortcut: Z (an arrow) must not move the cursor.
    if (state.ctrl) state.buttons &= ~arrows;
    const uint32_t pressed = state.buttons & ~last_buttons_;
    const uint32_t soft_down = soft_pressed;  // before key 5's EDIT handling below
    uint32_t held = state.buttons;
    if (on_song_view && (state.soft & 1u)) {
        if (soft_pressed & 1u) soft_edit_used_ = false;
        if (state.buttons & arrows) soft_edit_used_ = true;
        held |= ZT_BTN_EDIT;
        soft_pressed &= ~1u;  // acts on release
    }
    if (on_song_view && (soft_released & 1u) && !soft_edit_used_) soft_pressed |= 1u;
    const bool escape_pressed = state.escape && !last_escape_;
    const bool page_pressed = state.page && !last_page_;
    const bool page_released = !state.page && last_page_;
    last_buttons_ = state.buttons;
    last_escape_ = state.escape;
    last_page_ = state.page;

    // Characters newly pressed this poll (state.chars is every key held).
    std::string typed;
    for (char ch : state.chars) {
        if (last_chars_.find(ch) == std::string::npos) typed += ch;
    }
    last_chars_ = state.chars;

    // SYM+4..8: say what the key does here instead of pressing it.
    if (state.sym && (page_pressed || soft_down)) {
        int key = 4;
        for (int i = 0; i < 4; ++i) {
            if (soft_down & (1u << i)) key = 5 + i;
        }
        soft_edit_used_ = true;  // key 5 released after SYM is not EDIT
        show_key_hint(key);
        return;
    }
    if (hint_box_ && (pressed || page_pressed || soft_down || escape_pressed || !typed.empty())) {
        close_key_hint();
        return;
    }
    if (hint_box_ && lv_tick_get() - hint_shown_at_ >= 4000) close_key_hint();
    // Any key ends the instrument preview, ESC and the page keys included;
    // in the EQ editor only ESC and PLAY do, so the bands change while it
    // plays.
    const bool preview_key = eq_open_ ? escape_pressed
                                      : (pressed || page_pressed || soft_down || escape_pressed ||
                                         !typed.empty() || (state.enter && !last_enter_));
    if (instrument_preview_ && preview_key) {
        last_enter_ = state.enter;
        end_instrument_preview();
        redraw();
        return;
    }

    if (!volume_text_.empty() && lv_tick_get() - volume_shown_at_ >= 2000) {
        volume_text_.clear();
        redraw();
    }
    // Volume keys work everywhere in the app.
    for (auto [now, last, change] :
         {std::tuple{state.mute, &last_mute_, platform::volume::Change::ToggleMute},
          std::tuple{state.volume_down, &last_vol_down_, platform::volume::Change::Down},
          std::tuple{state.volume_up, &last_vol_up_, platform::volume::Change::Up}}) {
        if (now && !*last) {
            volume_text_ = platform::volume::apply(change);
            volume_shown_at_ = lv_tick_get();
            redraw();
        }
        *last = now;
    }

    const bool undo_pressed = state.undo && !last_undo_;
    const bool redo_pressed = state.redo && !last_redo_;
    last_undo_ = state.undo;
    last_redo_ = state.redo;
    const bool enter_pressed = state.enter && !last_enter_;
    last_enter_ = state.enter;

    // RENDER's modal takes every key: ESC or 8 cancels a running render,
    // any key closes the result.
    if ((render_modal_ || render_choice_ >= 0) &&
        (pressed || page_pressed || soft_pressed || escape_pressed || enter_pressed || !typed.empty())) {
        render_modal_key(pressed, enter_pressed, escape_pressed, soft_pressed,
                         pressed || page_pressed || soft_pressed || escape_pressed || enter_pressed || !typed.empty());
        return;
    }
    // MANUAL closes on any key.
    if (manual_open_ &&
        (pressed || page_pressed || soft_pressed || escape_pressed || enter_pressed || !typed.empty())) {
        manual_open_ = false;
        redraw();
        return;
    }
    // LIBRARY takes every key while it is open.
    const bool lib_question = lib_modal_ || lib_info_ || lib_confirm_remove_;
    if (library_open_ && (lib_question || !(page_pressed || state.page || page_released)) &&
        (pressed || page_pressed || soft_pressed || escape_pressed || enter_pressed || !typed.empty())) {
        library_input(pressed, enter_pressed, escape_pressed, soft_pressed,
                      pressed || page_pressed || soft_pressed || escape_pressed || enter_pressed || !typed.empty());
        redraw();
        return;
    }
    // REC. takes every key while it is open.
    if (rec_open_) {
        if (enter_pressed || (soft_pressed & 1u)) toggle_recording();
        else if (escape_pressed || (soft_pressed & 8u)) close_recorder(false);
        redraw();
        return;
    }
    // The EQ editor takes every key while it is open.
    if (eq_open_) {
        if (eq_editor_input(pressed, held, enter_pressed, escape_pressed, undo_pressed, soft_pressed,
                            typed, state.ctrl)) {
            redraw();
        }
        return;
    }
    // The sample editor takes every key while it is open.
    if (sample_edit_) {
        if (sample_editor_input(pressed, held, enter_pressed, escape_pressed, undo_pressed,
                                soft_pressed, typed, state.ctrl)) {
            redraw();
        }
        return;
    }
    // Ctrl shortcuts: Z undo, Y redo, S save, C copy, V paste.
    if (state.ctrl && !typed.empty()) {
        if (viewer_) {
            for (char ch : typed) {
                if (ch == 'Z' || ch == 'Y') zt_viewer_undo(viewer_, ch == 'Y' ? 1 : 0);
                else if (ch == 'S') {
                    volume_text_ = zt_viewer_save(viewer_) ? "SAVED" : "SAVE FAILED";
                    volume_shown_at_ = lv_tick_get();
                }
                else if (ch == 'C') zt_viewer_ux_copy(viewer_);
                else if (ch == 'V') zt_viewer_ux_paste(viewer_);
            }
        }
        redraw();
        return;
    }

    if (text_mode_) {
        if (escape_pressed) {
            text_mode_ = false;
        }
        else if (enter_pressed) {
            zt_viewer_set_text(viewer_, text_.c_str());
            text_mode_ = false;
        }
        else if (undo_pressed) {
            if (!text_.empty()) text_.pop_back();
        }
        else {
            for (char ch : typed) {
                if (static_cast<int>(text_.size()) < text_max_) text_ += ch;
            }
        }
        redraw();
        return;
    }
    if (viewer_ && confirm_close_) {
        // SAVE? ENTER (or 5) saves and closes, DEL (or 6) closes without
        // saving, ESC (or 8) keeps the song open.
        if (enter_pressed || (soft_pressed & 1u)) {
            if (zt_viewer_save(viewer_)) {
                close_song();
            }
            else {
                confirm_close_ = false;  // keep the song open, the save failed
                open_after_close_.clear();
            }
        }
        else if (undo_pressed || (soft_pressed & 2u)) {
            close_song();
        }
        else if (escape_pressed || (soft_pressed & 8u)) {
            confirm_close_ = false;
            open_after_close_.clear();
        }
        redraw();
        return;
    }
    // ENTER: open in LOAD; on a song view the field's own action; the song
    // name on the SONG page.
    if (enter_pressed) {
        if (browsing_) {
            open_entry();
        }
        else if (on_song_view && (zt_viewer_ux_state(viewer_) & ZT_UX_CONFIRM_OVERWRITE)) {
            zt_viewer_ux_save(viewer_);  // yes, overwrite
        }
        else if (on_song_view) {
            if (zt_viewer_ux_enter(viewer_) == 2) open_text_entry();
            after_input();
        }
        else if (viewer_ && page_ == ZT_PAGE_SONG) {
            // The name opens its text entry; SAVE, NEW, LOAD and RENDER act
            // as with EDIT.
            if (!open_text_entry()) {
                zt_viewer_input(viewer_, ZT_BTN_EDIT, ZT_BTN_EDIT);
                after_input();
            }
        }
        else if (viewer_ && page_ == ZT_PAGE_TRACKS) {
            zt_viewer_tracks_enter(viewer_);
            page_ = zt_viewer_page(viewer_);  // NAME opens the instrument on page 1
        }
        redraw();
        return;
    }
    // Key 4: next page on release after a tap; held 400 ms, the page before.
    if (page_pressed) {
        page_down_at_ = lv_tick_get();
        page_hold_done_ = false;
        return;
    }
    if (state.page) {
        if (!page_hold_done_ && lv_tick_get() - page_down_at_ >= 400) {
            page_hold_done_ = true;
            previous_page();
        }
        return;
    }
    if (page_released) {
        if (!page_hold_done_) next_page();
        return;
    }
    if (soft_pressed) {
        for (int i = 0; i < 4; ++i) {
            if (soft_pressed & (1u << i)) soft_key(i + 1);
        }
        redraw();
        return;
    }
    // ESC while editing a TRK pool value undoes the edit.
    if (escape_pressed && viewer_ && page_ == ZT_PAGE_TRACKS &&
        (zt_viewer_ux_state(viewer_) & ZT_UX_EDITING)) {
        zt_viewer_ux_back(viewer_);
        redraw();
        return;
    }
    if (escape_pressed && page_ != 0 && (viewer_ || page_ == kPageApp)) {
        // ESC on another page goes back to the TIMELINE (or the browser).
        page_ = 0;
        if (viewer_) zt_viewer_set_page(viewer_, 0);
        redraw();
        return;
    }
    if (escape_pressed && browsing_) {
        // ESC in LOAD goes back to HOME, or to the song it was opened from.
        end_browse();
        redraw();
        return;
    }
    // ESC on a song view: cancels an edit or a selection, else up a level.
    if (escape_pressed && on_song_view && zt_viewer_ux_back(viewer_)) {
        redraw();
        return;
    }
    if (escape_pressed && viewer_) {
        // Short ESC on the song level leaves the song for HOME.
        if (zt_viewer_dirty(viewer_)) {
            confirm_close_ = true;
        }
        else {
            close_song();
        }
        redraw();
        return;
    }
    if (page_ == kPageOscil) {
        return;
    }
    // DEL clears the field; Fn+DEL undoes (also Ctrl+Z).
    if (on_song_view && undo_pressed) {
        zt_viewer_ux_clear(viewer_);
        redraw();
        return;
    }
    if (viewer_ && page_ != kPageApp && redo_pressed) {
        zt_viewer_undo(viewer_, 0);
        redraw();
        return;
    }
    if (!pressed) {
        return;
    }
    if (instrument_preview_) {  // any key ends the instrument preview
        end_instrument_preview();
        redraw();
        return;
    }
    if (page_ == kPageApp) {
        handle_app(state.buttons, pressed);
    }
    else if (browsing_) {
        handle_browser(pressed);
    }
    else if (viewer_) {
        zt_viewer_input(viewer_, held, pressed);
        if (page_ == ZT_PAGE_TRACKS) page_ = zt_viewer_page(viewer_);  // SHIFT+RIGHT opens the instrument
        after_input();
    }
    else {
        handle_home(pressed);
    }
    redraw();
}

// EDIT alone on a NAME field (page 1 or SONG) opens the text entry.
bool TrackerScreen::open_text_entry() {
    if (!viewer_ || (page_ != ZT_PAGE_TIMELINE && page_ != ZT_PAGE_SONG)) {
        return false;
    }
    char current[64] = {};
    const int max = zt_viewer_text_target(viewer_, current, sizeof(current));
    if (max <= 0) {
        return false;
    }
    text_mode_ = true;
    text_ = current;
    while (!text_.empty() && text_.back() == ' ') text_.pop_back();
    text_max_ = max;
    return true;
}

void TrackerScreen::show_text_entry(std::string& line, int col, int& width) const {
    if (!text_mode_ || col < 0) return;
    std::string field = text_ + "_";
    width = std::max(width, text_max_ + 1);
    field.resize(static_cast<size_t>(width), ' ');
    if (line.size() < static_cast<size_t>(col + width)) line.resize(static_cast<size_t>(col + width), ' ');
    line.replace(static_cast<size_t>(col), static_cast<size_t>(width), field);
}

// Requests from the SONG page: LOAD closes the song, NEW opens a new file.
void TrackerScreen::after_input() {
    char path[1024] = {};
    const int request = zt_viewer_take_request(viewer_, path, sizeof(path));
    if (request == 0) {
        return;
    }
    if (request >= 0x100) {
        zt_viewer_stop(viewer_);
        char err[64] = {};
        if (zt_preview_play_instrument(preview_, viewer_, static_cast<uint8_t>(request & 0xFF), err,
                                       sizeof err)) {
            instrument_preview_ = true;
        }
        else {
            zt_viewer_attach_engine(viewer_, engine_, root_dir_.c_str());
        }
        return;
    }
    // INSTRUMENT: EDIT by the sample (5) opens the sample editor.
    if (request == 5) {
        open_sample_editor(path);
        return;
    }
    // INSTRUMENT: ENTER on EQ (6) opens the EQ editor.
    if (request == 6) {
        open_eq_editor();
        return;
    }
    if (request == 7) {  // REC. on an empty SAMPLE
        rec_open_ = true;
        rec_error_.clear();
        return;
    }
    if (request == 8) {  // SONG: RENDER (again while running: cancel)
        if (render_ && zt_render_status(render_, nullptr, nullptr, nullptr) == ZT_RENDER_RUNNING) {
            zt_render_free(render_);
            render_ = nullptr;
            render_text_ = tr({"STOP", "STOP", "STOP", "STOP", "PARADO", "中止", "已停止"});
            render_failed_ = true;
        }
        else {
            render_choice_ = 2;  // the modal asks first: MIX, STEMS or both
        }
        return;
    }
    // INSTRUMENT: LOAD (3) or the sample (4) opens LOAD for that kind.
    if (request == 3 || request == 4) {
        start_browse(request == 3 ? BrowserView::Kind::Instrument : BrowserView::Kind::Sample);
        return;
    }
    open_after_close_ = request == 2 ? path : "";
    if (zt_viewer_dirty(viewer_)) {
        confirm_close_ = true;
    }
    else {
        close_song();
    }
}

void TrackerScreen::close_song() {
    // A RENDER of this song stops with it.
    zt_render_free(render_);
    render_ = nullptr;
    render_text_.clear();
    render_failed_ = false;
    render_modal_ = false;
    render_choice_ = -1;
    zt_viewer_stop(viewer_);
    zt_viewer_close(viewer_);
    viewer_ = nullptr;
    confirm_close_ = false;
    page_ = 0;
    browsing_ = false;
    if (!open_after_close_.empty()) {
        const std::string path = open_after_close_;
        open_after_close_.clear();
        open_song(path);
        list_directory();
    }
}

void TrackerScreen::list_directory() {
    namespace fs = std::filesystem;
    entries_.clear();
    std::error_code ec;
    for (const auto& e : fs::directory_iterator(dir_, ec)) {
        const auto name = e.path().filename().string();
        if (name.empty() || name[0] == '.') {
            continue;
        }
        using Kind = BrowserView::Kind;
        if (e.is_directory(ec)) {
            entries_.push_back({name, Kind::Folder, ""});
            continue;
        }
        auto ext = e.path().extension().string();
        std::transform(ext.begin(), ext.end(), ext.begin(), ::tolower);
        const Kind kind = ext == ".m8s" ? Kind::Song : ext == ".m8i" ? Kind::Instrument
                        : ext == ".wav" ? Kind::Sample : Kind::Parent;
        if (kind == Kind::Parent) continue;  // not an M8 file
        // Size: bytes up to 999, then K and M.
        const auto bytes = e.file_size(ec);
        char size[16];
        if (ec) size[0] = 0;
        else if (bytes < 1000) std::snprintf(size, sizeof size, "%lluB", static_cast<unsigned long long>(bytes));
        else if (bytes < 1000 * 1000) std::snprintf(size, sizeof size, "%lluK", static_cast<unsigned long long>((bytes + 999) / 1000));
        else std::snprintf(size, sizeof size, "%.1fM", static_cast<double>(bytes) / 1e6);
        ec.clear();
        entries_.push_back({name, kind, size});
    }
    // Folders first, then songs, instruments and samples, each by name.
    std::sort(entries_.begin(), entries_.end(), [](const Entry& a, const Entry& b) {
        return a.kind != b.kind ? a.kind < b.kind : a.name < b.name;
    });
    if (dir_ != root_dir_) {
        entries_.insert(entries_.begin(), {"..", BrowserView::Kind::Parent, ""});
    }
    error_ = ec ? "cannot read " + dir_ : "";
    selected_ = 0;
    top_ = 0;
}

void TrackerScreen::open_entry() {
    if (entries_.empty()) {
        return;
    }
    const auto& e = entries_[selected_];
    namespace fs = std::filesystem;
    if (!openable(e)) {
        return;
    }
    if (e.directory()) {
        dir_ = e.name == ".." ? fs::path(dir_).parent_path().string()
                              : (fs::path(dir_) / e.name).string();
        list_directory();
        return;
    }
    const std::string path = (fs::path(dir_) / e.name).string();
    if (e.kind != BrowserView::Kind::Song) {
        // An instrument or a sample goes into the INSTRUMENT slot LOAD was
        // opened from (ZT_BROWSE alone has no slot to fill).
        if (!viewer_) {
            browser_message_ = tr({"open a song first", "apri una canzone", "ouvre d'abord un morceau", "erst einen Song öffnen", "abre una canción primero", "先に曲を開く", "请先打开歌曲"});
            browser_message_at_ = lv_tick_get();
            return;
        }
        const bool ok = e.kind == BrowserView::Kind::Instrument
                            ? zt_viewer_load_instrument(viewer_, path.c_str())
                            : zt_viewer_set_sample(viewer_, path.c_str());
        if (ok) {
            preview_took_engine_ = false;  // loading published the song again
            end_browse();
        }
        else {
            browser_message_ = tr({"cannot load", "non caricabile", "chargement impossible", "nicht ladbar", "no se puede cargar", "読み込めません", "无法载入"});
            browser_message_at_ = lv_tick_get();
        }
        return;
    }
    open_song(path);
}

void TrackerScreen::open_song(const std::string& path) {
    char err[256] = {};
    // The song playing as a preview is already loaded: open it as it is.
    viewer_ = zt_preview_take_song(preview_, path.c_str());
    const bool preloaded = viewer_ != nullptr;
    stop_preview();
    if (!viewer_) viewer_ = zt_viewer_open(path.c_str(), err, sizeof(err));
    error_ = viewer_ ? "" : err;
    page_ = 0;
    if (viewer_ && engine_ && !preloaded) {
        zt_viewer_attach_engine(viewer_, engine_, root_dir_.c_str());
    }
    if (viewer_) {
        browsing_ = false;
        if (settings_.last_song != path) {
            settings_.last_song = path;
            save_settings();
        }
    }
    else {
        browser_message_ = error_;
        browser_message_at_ = lv_tick_get();
    }
}

// What LOAD can open: folders and the kind it was opened for (songs from
// HOME; instruments and samples once those pages load them).
bool TrackerScreen::openable(const Entry& e) const {
    return e.directory() || e.kind == browse_for_;
}

bool TrackerScreen::previewable(const Entry& e) const {
    return !e.directory() && e.kind == browse_for_;
}

void TrackerScreen::stop_preview() {
    zt_preview_stop(preview_);
}

void TrackerScreen::end_instrument_preview() {
    instrument_preview_ = false;
    zt_preview_stop(preview_);
    if (viewer_) zt_viewer_attach_engine(viewer_, engine_, root_dir_.c_str());
}

void TrackerScreen::start_browse(BrowserView::Kind kind) {
    if (viewer_) zt_viewer_stop(viewer_);
    browse_for_ = kind;
    browsing_ = true;
    const char* folder = kind == BrowserView::Kind::Instrument ? "Instruments"
                       : kind == BrowserView::Kind::Sample     ? "Samples"
                                                               : "";
    std::error_code ec;
    const auto start = std::filesystem::path(root_dir_) / folder;
    dir_ = *folder && std::filesystem::is_directory(start, ec) ? start.string() : root_dir_;
    list_directory();
}

void TrackerScreen::end_browse() {
    stop_preview();
    browsing_ = false;
    if (viewer_) {
        // A song or instrument preview replaced the song in the engine.
        if (preview_took_engine_) zt_viewer_attach_engine(viewer_, engine_, root_dir_.c_str());
        preview_took_engine_ = false;
        browse_for_ = BrowserView::Kind::Song;
    }
}

// Key 6 in LOAD: STOP when a preview plays, else PLAY the selected file.
void TrackerScreen::toggle_preview() {
    if (zt_preview_playing(preview_)) {
        stop_preview();
        return;
    }
    if (entries_.empty() || !previewable(entries_[selected_])) return;
    const auto& e = entries_[selected_];
    const int kind = e.kind == BrowserView::Kind::Song         ? ZT_PREVIEW_SONG
                   : e.kind == BrowserView::Kind::Instrument ? ZT_PREVIEW_INSTRUMENT
                                                              : ZT_PREVIEW_SAMPLE;
    char err[128] = {};
    const std::string path = (std::filesystem::path(dir_) / e.name).string();
    if (viewer_ && kind != ZT_PREVIEW_SAMPLE) preview_took_engine_ = true;
    if (!zt_preview_play(preview_, path.c_str(), kind, err, sizeof err)) {
        browser_message_ = err;
        browser_message_at_ = lv_tick_get();
    }
}

// HOME: RESUME the last song, LOAD (the browser), NEW.
std::array<HomeView::Item, 4> TrackerScreen::home_items() const {
    namespace fs = std::filesystem;
    std::error_code ec;
    const bool can_resume = !settings_.last_song.empty() && fs::is_regular_file(settings_.last_song, ec);
    std::string next = "NEW00.m8s";
    for (int n = 0; n < 100; ++n) {
        char name[16];
        std::snprintf(name, sizeof name, "NEW%02d.m8s", n);
        if (!fs::exists(fs::path(root_dir_) / name, ec)) {
            next = name;
            break;
        }
    }
    return {{
        {"\uE3D2", tr({"RESUME", "RIAPRI", "REPRENDRE", "WEITER", "REANUDAR", "再開", "继续"}),
         can_resume ? fs::path(settings_.last_song).stem().string() : tr({"no session", "nessuna", "aucune session", "keine Sitzung", "sin sesión", "セッションなし", "无会话"}),
         can_resume},
        {"\uE256", tr({"LOAD", "CARICA", "CHARGER", "LADEN", "CARGAR", "読込", "载入"}), fs::path(root_dir_).filename().string(), true},
        {"\uE3D6", tr({"NEW", "NUOVA", "NOUVEAU", "NEU", "NUEVO", "新規", "新建"}), home_message_.empty() ? next : home_message_, true},
        {"\uE758", "LIBRARY", tr({"packs", "pacchetti", "packs", "Pakete", "paquetes", "パック", "资源包"}), true},
    }};
}

void TrackerScreen::home_action(int item) {
    const auto items = home_items();
    if (item < 0 || item > 3 || !items[item].enabled) return;
    if (item == 3) {
        open_library();
        return;
    }
    home_message_.clear();
    if (item == 0) {
        open_song(settings_.last_song);
    }
    else if (item == 1) {
        browsing_ = true;
        list_directory();
    }
    else {
        char out[512] = {};
        if (zt_new_song(root_dir_.c_str(), root_dir_.c_str(), out, sizeof out)) {
            open_song(out);
        }
        else {
            home_message_ = out;
        }
    }
}

// HOME has no cursor: RESUME, LOAD and NEW are keys
// 6-8 only, so the arrows and EDIT do nothing here.
void TrackerScreen::handle_home(uint32_t) {}

void TrackerScreen::handle_browser(uint32_t pressed) {
    const int n = static_cast<int>(entries_.size());
    // A new selection (or folder) ends the preview of the old one.
    if (pressed & (ZT_BTN_UP | ZT_BTN_DOWN | ZT_BTN_LEFT | ZT_BTN_RIGHT | ZT_BTN_EDIT)) stop_preview();
    if (pressed & ZT_BTN_UP) selected_ = std::max(0, selected_ - 1);
    if (pressed & ZT_BTN_DOWN) selected_ = std::min(std::max(0, n - 1), selected_ + 1);
    if (pressed & (ZT_BTN_EDIT | ZT_BTN_RIGHT)) {
        open_entry();
        return;
    }
    if ((pressed & ZT_BTN_LEFT) && dir_ != root_dir_) {
        dir_ = std::filesystem::path(dir_).parent_path().string();
        list_directory();
    }
}

// APP page: UP/DOWN pick a setting, LEFT/RIGHT (alone or with EDIT) change it;
// past LANGUAGE, DOWN scrolls into the instructions (help_text.h).
namespace {
constexpr int kAppFields = 4;
}  // namespace


// APP page: UP/DOWN pick a setting (past the last one they scroll the
// instructions), LEFT/RIGHT (alone or with EDIT) change it.
void TrackerScreen::handle_app(uint32_t, uint32_t pressed) {
    // Past LANGUAGE, DOWN scrolls the page (settings, then the instructions).
    const int step_px = 24;
    const int limit = std::max(0, app_view_.content_height() - app_view_.height());
    if (pressed & ZT_BTN_UP) {
        if (app_scroll_ > 0) app_scroll_ = std::max(0, app_scroll_ - step_px);
        else app_field_ = std::max(0, app_field_ - 1);
    }
    if (pressed & ZT_BTN_DOWN) {
        if (app_field_ < kAppFields - 1) ++app_field_;
        else app_scroll_ = std::min(limit, app_scroll_ + step_px);
    }
    const int step = (pressed & ZT_BTN_RIGHT) ? 1 : (pressed & ZT_BTN_LEFT) ? -1 : 0;
    if (step == 0) {
        return;
    }
    if (app_field_ == 0) {
        auto it = std::find(kFontSizes.begin(), kFontSizes.end(), settings_.font_size);
        int i = it == kFontSizes.end() ? 3 : static_cast<int>(it - kFontSizes.begin());
        i = std::clamp(i + step, 0, static_cast<int>(kFontSizes.size()) - 1);
        if (kFontSizes[i] == settings_.font_size) return;
        settings_.font_size = kFontSizes[i];
    }
    else if (app_field_ == 1) {
        const int n = static_cast<int>(kThemeNames.size());
        settings_.theme = (settings_.theme + step + n) % n;
    }
    else if (app_field_ == 2) {
        settings_.spectrum = !settings_.spectrum;
    }
    else {
        settings_.language = (settings_.language + step + zt::i18n::kCount) % zt::i18n::kCount;
        apply_language();
        app_scroll_ = 0;
    }
    save_settings();
    build_grid();
}


void TrackerScreen::update_bar() {
    char header[64] = " ZeroTracker";
    if (viewer_) {
        header[0] = ' ';
        zt_viewer_header(viewer_, header + 1, sizeof(header) - 1);
    }
    const bool show_volume =
        !volume_text_.empty() && lv_tick_get() - volume_shown_at_ < 2000;
    // The header is "NAME  BPM 140": the name goes left, the tempo right.
    std::string name = header, bpm;
    if (const auto at = name.find("  BPM "); at != std::string::npos) {
        bpm = name.substr(at + 2);
        name.resize(at);
    }
    const bool message = show_volume;
    const std::string bar = show_volume ? " " + volume_text_ : name;
    if (bar != shown_bar_) {
        lv_label_set_text(bar_left_, bar.c_str());
        shown_bar_ = bar;
        // A message stands out from the song name: on the alert colour (a
        // question) or the accent (the volume), with its own text colour.
        const Theme& t = theme();
        const uint32_t bg = confirm_close_ ? t.alert : t.accent;
        lv_obj_set_style_bg_color(bar_left_, lv_color_hex(bg), 0);
        lv_obj_set_style_bg_opa(bar_left_, message ? LV_OPA_COVER : LV_OPA_TRANSP, 0);
        lv_obj_set_style_radius(bar_left_, 3, 0);
        lv_obj_set_style_pad_right(bar_left_, message ? 4 : 0, 0);
        lv_obj_set_style_text_color(bar_left_, lv_color_hex(!message ? t.hi : confirm_close_ ? t.on_alert : t.bg), 0);
    }
    if (message) bpm.clear();
    if (bpm != shown_bpm_) {
        lv_label_set_text(bar_bpm_, bpm.c_str());
        lv_obj_align(bar_bpm_, LV_ALIGN_TOP_RIGHT, -96, 0);
        shown_bpm_ = bpm;
    }

    // Battery: read every 10 s, it is a sysfs read.
    const uint32_t now = lv_tick_get();
    if (battery_checked_at_ == 0 || now - battery_checked_at_ > 10000) {
        battery_checked_at_ = now == 0 ? 1 : now;
        const auto status = platform::read_device_status();
        if (status.battery_present && status.battery_percent >= 0) {
            const int pct = static_cast<int>(status.battery_percent);
            lv_label_set_text_fmt(bar_right_, "%s%d%%", status.battery_charging ? "+" : "", pct);
            // Green from 60%, yellow from 30%, orange from 15%, red below.
            const auto& b = theme().battery;
            const uint32_t color = pct >= 60 ? b[0] : pct >= 30 ? b[1] : pct >= 15 ? b[2] : b[3];
            lv_obj_set_style_text_color(bar_right_, lv_color_hex(color), 0);
        }
        else {
            lv_label_set_text(bar_right_, "");
        }
    }
}

// The buttons of keys 5-8 on the current page (slot 0 is the page itself).
std::array<TrackerScreen::NavSlot, 5> TrackerScreen::nav_slots() const {
    std::array<NavSlot, 5> slots{};
    slots[0] = {"", false, true};
    const bool playing = viewer_ && zt_viewer_playing(viewer_);
    const char* play = playing ? "STOP" : "PLAY";
    // Button names follow the APP language, at most 6 characters in every
    // language (longer ones crowd a fifth of the bar).
    // Music terms (MUTE, SOLO...) stay as they are. The key is the English
    // name the pages use; the first text is what English shows.
    auto tr = [this](const char* name) -> const char* {
        if (!name) return name;
        struct Word {
            const char* key;
            zt::i18n::Text text;
        };
        static const std::array<Word, 36> words{{
            {"PREVIEW", {"LISTEN", "SENTI", "ÉCOUTE", "HÖREN", "OÍR", "試聴", "试听"}},
            {"START", {"START", "AVVIA", "DÉPART", "START", "INICIO", "開始", "开始"}},
            {"EDIT", {"EDIT", "CAMBIA", "MODIF.", "ÄNDERN", "EDITAR", "編集", "编辑"}},
            {"OPEN", {"OPEN", "APRI", "OUVRIR", "ÖFFNEN", "ABRIR", "開く", "打开"}},
            {"SELECT", {"SELECT", "SELEZ", "SÉLECT", "WAHL", "SELEC.", "選択", "选择"}},
            {"PLAY", {"PLAY", "SUONA", "JOUER", "START", "TOCAR", "再生", "播放"}},
            {"DONE", {"DONE", "OK", "OK", "OK", "OK", "完了", "完成"}},
            {"CANCEL", {"CANCEL", "ANNUL.", "ANNUL.", "ZURÜCK", "CANCEL", "取消", "取消"}},
            {"CHOOSE", {"CHOOSE", "SCEGLI", "CHOIX", "WÄHLEN", "ELEGIR", "選ぶ", "选取"}},
            {"COPY", {"COPY", "COPIA", "COPIER", "KOPIE", "COPIAR", "コピー", "复制"}},
            {"PASTE", {"PASTE", "INCOL.", "COLLER", "EINFÜG", "PEGAR", "貼付", "粘贴"}},
            {"LOAD", {"LOAD", "CARICA", "OUVRIR", "LADEN", "CARGAR", "読込", "载入"}},
            {"SAVE", {"SAVE", "SALVA", "ENREG.", "SPEIC.", "GUARDA", "保存", "保存"}},
            {"NEW", {"NEW", "NUOVA", "NOUV.", "NEU", "NUEVO", "新規", "新建"}},
            {"RESUME", {"RESUME", "RIAPRI", "REPR.", "WEITER", "SEGUIR", "再開", "继续"}},
            {"UP", {"UP", "SU", "HAUT", "AUF", "ARRIBA", "上", "上"}},
            {"DISCARD", {"DROP", "SCARTA", "JETER", "WEG", "TIRAR", "破棄", "放弃"}},
            {"LIMIT", {"LIMIT", "LIMITE", "LIMITE", "LIMIT", "LÍMITE", "リミット", "限制"}},
            {"VIEW", {"VIEW", "VISTA", "VUE", "SICHT", "VISTA", "表示", "视图"}},
            {"THEME", {"THEME", "TEMA", "THÈME", "THEMA", "TEMA", "テーマ", "主题"}},
            {"YES", {"YES", "SI", "OUI", "JA", "SÍ", "はい", "是"}},
            {"APPLY", {"APPLY", "APPL.", "APPLIQ", "ANWEND", "APLICA", "適用", "应用"}},
            {"CLOSE", {"CLOSE", "CHIUDI", "FERMER", "ZU", "CERRAR", "閉じる", "关闭"}},
            {"MODE", {"MODE", "MODO", "MODE", "MODUS", "MODO", "モード", "模式"}},
            {"PROCESS", {"ACTION", "AZIONI", "TRAIT.", "BEARB.", "PROC.", "処理", "处理"}},
            {"TYPE", {"TYPE", "TIPO", "TYPE", "TYP", "TIPO", "タイプ", "类型"}},
            {"RESET", {"RESET", "AZZERA", "RÉINIT", "RESET", "REINIC", "リセット", "重置"}},
            {"STOP", {"STOP", "STOP", "STOP", "STOP", "PARAR", "停止", "停止"}},
            {"RENDER", {"RENDER", "RENDER", "RENDU", "RENDER", "RENDER", "書出", "渲染"}},
            {"LIB", {"LIB", "LIB", "BIBL.", "BIBL.", "BIBL.", "ライブ", "资源"}},
            {"GET", {"GET", "PRENDI", "OBTEN.", "HOLEN", "BAJAR", "取得", "获取"}},
            {"REMOVE", {"REMOVE", "TOGLI", "SUPPR.", "ENTF.", "QUITAR", "削除", "删除"}},
            {"INFO", {"INFO", "INFO", "INFO", "INFO", "INFO", "情報", "信息"}},
            {"RELOAD", {"RELOAD", "RICAR.", "RECH.", "NEU", "RECAR.", "更新", "刷新"}},
            {"NO", {"NO", "NO", "NON", "NEIN", "NO", "いいえ", "否"}},
            {"MANUAL", {"MANUAL", "GUIDA", "GUIDE", "HILFE", "GUÍA", "手引き", "手册"}},
        }};
        for (const auto& w : words) {
            if (std::strcmp(w.key, name) == 0) return zt::i18n::pick(w.text, lang());
        }
        return name;
    };
    auto set = [&](const char* a, const char* b, const char* c, const char* d) {
        const char* names[4] = {tr(a), tr(b), tr(c), tr(d)};
        for (int i = 0; i < 4; ++i) {
            const char* keys[4] = {a, b, c, d};
            slots[i + 1] = {names[i] ? names[i] : "", false, names[i] != nullptr, keys[i] ? keys[i] : ""};
        }
    };
    if (rec_open_) {
        set(platform::audio_input::recording() ? "STOP" : "START", nullptr, nullptr, "CANCEL");
    }
    else if (eq_open_) {
        set("TYPE", "MODE", "RESET", viewer_ && zt_viewer_playing(viewer_) ? "STOP" : "PLAY");
    }
    else if (sample_edit_) {
        if (se_menu_) set("APPLY", nullptr, nullptr, "CLOSE");
        else if (se_naming_) set("SAVE", nullptr, nullptr, "CANCEL");
        else if (se_confirm_exit_) set("SAVE", "DISCARD", nullptr, "CANCEL");
        else set("MODE", "PROCESS", se_playing_ ? "STOP" : "PLAY", "SAVE");
    }
    else if (render_choice_ >= 0) {
        set("START", nullptr, nullptr, "CANCEL");
    }
    else if (render_modal_) {
        const bool running = render_ && zt_render_status(render_, nullptr, nullptr, nullptr) == ZT_RENDER_RUNNING;
        set(nullptr, nullptr, nullptr, running ? "CANCEL" : "CLOSE");
    }
    else if (confirm_close_) {
        set("SAVE", "DISCARD", nullptr, "CANCEL");
    }
    else if (library_open_) {
        uint32_t permille = 0;
        const uint8_t state = library_ ? zt_library_status(library_, &permille, nullptr, 0) : ZT_LIB_FAILED;
        const bool busy = state == ZT_LIB_DOWNLOADING || state == ZT_LIB_INSTALLING;
        ZtPackInfo info{};
        const bool have = library_pack(lib_selected_, info);
        if (lib_modal_) set(nullptr, nullptr, nullptr, busy ? "CANCEL" : "CLOSE");
        else if (lib_info_) set(nullptr, nullptr, nullptr, "CLOSE");
        else if (lib_confirm_remove_) set("YES", nullptr, nullptr, "NO");
        else set(have ? (info.installed ? "REMOVE" : "GET") : nullptr, have ? "INFO" : nullptr, "RELOAD", nullptr);
    }
    else if (manual_open_) {
        set(nullptr, nullptr, nullptr, "CLOSE");
    }
    else if (page_ == kPageApp) {
        set("FONT-", "FONT+", "THEME", "MANUAL");
    }
    else if (page_ == kPageOscil) {
        set(nullptr, nullptr, nullptr, "VIEW");
    }
    else if (browsing_) {
        const bool can_open = !entries_.empty() && openable(entries_[selected_]);
        const bool playing = zt_preview_playing(preview_);
        const bool can_play = playing || (!entries_.empty() && previewable(entries_[selected_]));
        // 5 goes up a folder; 8 leaves LOAD: back to HOME, or, from a song
        // (an instrument or a sample to pick), CANCEL back to it.
        set(dir_ != root_dir_ ? "UP" : nullptr, can_play ? (playing ? "STOP" : "PLAY") : nullptr,
            can_open ? "OPEN" : nullptr, viewer_ ? "CANCEL" : "HOME");
    }
    else if (!viewer_) {
        set("LIB", home_items()[0].enabled ? "RESUME" : nullptr, "LOAD", "NEW");
    }
    else {
        switch (page_) {
        case ZT_PAGE_MIXER: set("MUTE", "SOLO", "LIMIT", "OSCIL"); break;
        case ZT_PAGE_TRACKS: {
            // MUTE and SOLO on a track card; the POOL has no track.
            ZtTracksPage p{};
            const bool on_track = zt_viewer_tracks_page(viewer_, &p) && p.cursor < 8;
            set("OPEN", on_track ? "MUTE" : nullptr, on_track ? "SOLO" : nullptr, play);
            break;
        }
        case ZT_PAGE_SONG: set("SAVE", "RENDER", "LOAD", play); break;
        default: {
            // Song views (TIME, CHAIN/PHRASE, INSTRUMENT, TABLE).
            const uint32_t ux = zt_viewer_ux_state(viewer_);
            if (ux & ZT_UX_CONFIRM_OVERWRITE) set("YES", nullptr, nullptr, "NO");
            else if (ux & ZT_UX_EDITING) set("DONE", nullptr, nullptr, "CANCEL");
            else if (ux & ZT_UX_PICKER) set("CHOOSE", nullptr, nullptr, "CANCEL");
            else if (ux & ZT_UX_SELECTING) set("COPY", "PASTE", nullptr, "CANCEL");
            else if (ux & ZT_UX_INSTRUMENT) set("EDIT", "LOAD", "SAVE", play);
            else set("EDIT", (ux & ZT_UX_CAN_OPEN) ? "OPEN" : nullptr, "SELECT", play);
            break;
        }
        }
    }
    return slots;
}

void TrackerScreen::update_bottom_bar() {
    const auto slots = nav_slots();
    const Theme& t = theme();
    for (size_t i = 0; i < slots.size(); ++i) {
        std::string text;
        if (i == 0 && page_ == kPageOscil) {
            text = "MIX";  // OSCIL belongs to MIX: key 4 goes back to it
        }
        else if (i == 0) {
            text = library_open_ ? "LIB"
                   : rec_open_    ? "REC"
                   : eq_open_     ? "EQ"
                   : sample_edit_ ? "SAMPLE"
                   : browsing_  ? "LOAD"
                   : viewer_ || page_ == kPageApp ? kPageNames[page_]
                                                  : "HOME";
        }
        else {
            text = slots[i].text;
        }
        const lv_font_t* font = slots[i].icon && icon_font_ ? icon_font_ : nav_font_;
        const std::string key = text + (slots[i].enabled ? "1" : "0");
        if (key == nav_shown_[i]) continue;
        nav_shown_[i] = key;
        auto* l = nav_labels_[i];
        lv_obj_set_style_text_font(l, font, 0);
        // White over the spectrum. The page name is a tab: dark text on the
        // theme's bright colour, so it reads as a name and not as a button.
        if (i == 0) {
            lv_obj_set_style_text_color(l, lv_color_hex(t.bg), 0);
            lv_obj_set_style_bg_color(l, lv_color_hex(t.playing), 0);
            lv_obj_set_style_bg_opa(l, text.empty() ? LV_OPA_TRANSP : LV_OPA_COVER, 0);
        }
        else {
            lv_obj_set_style_text_color(l, lv_color_hex(slots[i].enabled ? t.hi : t.dim), 0);
        }
        lv_label_set_text(l, text.c_str());
        // Centre the glyphs (and the tab's padding) vertically in the bar.
        const int h = lv_font_get_line_height(font) + (i == 0 ? 2 * kTabPadV : 0);
        lv_obj_set_y(l, view::kScreenHeight - kNavHeight + std::max(0, (kNavHeight - h) / 2));
    }
}

// Keys 5-8 off page 1: the page's buttons.
void TrackerScreen::soft_key(int slot) {
    if (page_ == kPageApp && slot == 4) {
        manual_open_ = true;
        return;
    }
    if (page_ == kPageApp) {
        const int field = app_field_;
        if (slot == 1 || slot == 2) {
            app_field_ = 0;
            handle_app(0, slot == 1 ? ZT_BTN_LEFT : ZT_BTN_RIGHT);
        }
        else if (slot == 3) {
            app_field_ = 1;
            handle_app(0, ZT_BTN_RIGHT);
        }
        app_field_ = field;
        return;
    }
    if (!viewer_ || browsing_) {
        if (!browsing_) {
            // Keys 6-8: RESUME, LOAD, NEW; key 5: LIBRARY.
            home_action(slot == 1 ? 3 : slot - 2);
        }
        else if (slot == 1 && dir_ != root_dir_) {
            stop_preview();
            dir_ = std::filesystem::path(dir_).parent_path().string();
            list_directory();
        }
        else if (slot == 2) {
            toggle_preview();
        }
        else if (slot == 3) {
            open_entry();
        }
        else if (slot == 4) {
            end_browse();
        }
        return;
    }
    // OSCIL opens the oscilloscopes, MIX there goes back.
    if (page_ == ZT_PAGE_MIXER && slot == 4) {
        page_ = kPageOscil;
        return;
    }
    if (page_ == kPageOscil) {
        if (slot == 4) scopes_.next_layout();  // VIEW
        return;
    }
    if (page_ == ZT_PAGE_TIMELINE) {
        // Song views: the actions named in the bar (see nav_slots).
        const uint32_t ux = zt_viewer_ux_state(viewer_);
        if (ux & ZT_UX_CONFIRM_OVERWRITE) {
            if (slot == 1) zt_viewer_ux_save(viewer_);
            else if (slot == 4) zt_viewer_ux_back(viewer_);
        }
        else if (ux & (ZT_UX_EDITING | ZT_UX_PICKER)) {
            if (slot == 1) zt_viewer_ux_enter(viewer_);  // DONE / CHOOSE
            else if (slot == 4) zt_viewer_ux_back(viewer_);  // CANCEL
        }
        else if (ux & ZT_UX_SELECTING) {
            if (slot == 1) zt_viewer_ux_copy(viewer_);
            else if (slot == 2) zt_viewer_ux_paste(viewer_);
            else if (slot == 4) zt_viewer_ux_back(viewer_);
        }
        else if (slot == 1) {
            zt_viewer_ux_edit(viewer_);
        }
        else if (slot == 2) {
            if (ux & ZT_UX_INSTRUMENT) zt_viewer_ux_load(viewer_);
            else zt_viewer_ux_open(viewer_);
        }
        else if (slot == 3) {
            if (ux & ZT_UX_INSTRUMENT) zt_viewer_ux_save(viewer_);
            else zt_viewer_ux_select(viewer_);
        }
        else if (slot == 4) {
            zt_viewer_input(viewer_, ZT_BTN_PLAY, ZT_BTN_PLAY);
        }
        after_input();
        return;
    }
    zt_viewer_soft_key(viewer_, slot);
    page_ = zt_viewer_page(viewer_);  // INST's OPEN goes to the instrument on page 1
    after_input();
}

void TrackerScreen::update_meters() {
    uint16_t levels[8] = {};
    if (viewer_) {
        zt_viewer_levels(viewer_, levels);
    }
    // dB scale, -60..0 dBFS: M8 track peaks sit around -40 dBFS.
    std::vector<int> px(8, 0);
    for (size_t t = 0; t < px.size(); ++t) {
        if (levels[t] > 0) {
            const double db = 20.0 * std::log10(levels[t] / 65535.0);
            px[t] = static_cast<int>(std::clamp((db + 60.0) / 60.0, 0.0, 1.0) * meters_.height());
        }
    }
    meters_.set_heights(px);
}

// Spectrum in the top band at the redraw rate (about 20 Hz while a song is
// open); heights in 2 px steps so a still bar does not redraw.
void TrackerScreen::update_spectrum() {
    if (!spectrum_ || !settings_.spectrum) {
        return;
    }
    const bool fresh = engine_ && viewer_ && zt_viewer_playing(viewer_) &&
                       zt_engine_read_output(engine_, tap_.data(), tap_.size()) == tap_.size();
    const auto& levels = fresh ? spectrum_->update(tap_.data(), 0.06f) : spectrum_->decay(0.06f);
    std::vector<int> px(levels.size());
    for (size_t b = 0; b < levels.size(); ++b) {
        px[b] = static_cast<int>(levels[b] * spectrum_bars_.height());
    }
    spectrum_bars_.set_heights(px);
}

void TrackerScreen::update_mixer_bars() {
    int32_t area[4] = {};
    const bool show = viewer_ && page_ == ZT_PAGE_MIXER && zt_viewer_meter_area(viewer_, area);
    mixer_bars_.set_visible(show);
    if (!show) {
        return;
    }
    if (!std::equal(std::begin(area), std::end(area), mixer_area_.begin())) {
        std::copy(std::begin(area), std::end(area), mixer_area_.begin());
        const int y0 = top_px_ + area[0] * cell_h_;
        const int height = (area[1] - area[0] + 1) * cell_h_ - 2;
        mixer_bars_.set_area(0, y0, view::kScreenWidth, height);
        std::vector<int> x, w;
        for (int s = 0; s < 9; ++s) {
            x.push_back((area[2] + s * area[3]) * cell_w_);
            w.push_back(2 * cell_w_ - 2);
        }
        mixer_bars_.set_layout(std::move(x), std::move(w));
    }
    uint16_t levels[8] = {};
    zt_viewer_levels(viewer_, levels);
    const uint16_t master = *std::max_element(std::begin(levels), std::end(levels));
    std::vector<int> px(9, 0);
    for (size_t s = 0; s < px.size(); ++s) {
        const uint16_t level = s < 8 ? levels[s] : master;
        if (level > 0) {
            const double db = 20.0 * std::log10(level / 65535.0);
            px[s] = static_cast<int>(std::clamp((db + 60.0) / 60.0, 0.0, 1.0) * mixer_bars_.height());
        }
    }
    mixer_bars_.set_heights(px);
}

// Highlights: overlay labels with the span's text in its style. They sit on
// an opaque background (the cursor colour where the cursor is) so the plain
// glyph below does not show through.
void TrackerScreen::draw_highlights(const std::vector<std::string>& lines, int cursor_line,
                                    int cursor_col, int cursor_width) {
    int32_t spans[96 * 4] = {};
    size_t n = 0;
    if (viewer_ && page_ != kPageApp) {
        n = zt_viewer_highlights(viewer_, rows_count_, cols_, spans, overlays_.size());
    }
    const Theme& t = theme();
    for (size_t i = 0; i < overlays_.size(); ++i) {
        Overlay& o = overlays_[i];
        if (i >= n) {
            if (o.visible) {
                lv_obj_add_flag(o.label, LV_OBJ_FLAG_HIDDEN);
                o.visible = false;
            }
            continue;
        }
        const int line = spans[i * 4], col = spans[i * 4 + 1], width = spans[i * 4 + 2];
        const int style = spans[i * 4 + 3];
        const std::string& src = line < static_cast<int>(lines.size()) ? lines[line] : "";
        const std::string text = col < static_cast<int>(src.size()) ? src.substr(col, width) : "";
        const bool on_cursor = line == cursor_line && col < cursor_col + cursor_width &&
                               col + width > cursor_col;
        const uint32_t bg = on_cursor ? t.cursor : t.bg;
        const int x = col * cell_w_, y = top_px_ + line * cell_h_;
        if (!o.visible || o.text != text || o.x != x || o.y != y || o.style != style || o.bg != bg) {
            uint32_t color = t.fg;
            switch (style) {
            case ZT_STYLE_PLAYING: color = t.playing; break;
            case ZT_STYLE_ACCENT: color = t.accent; break;
            case ZT_STYLE_MUTED: color = t.muted; break;
            case ZT_STYLE_SOLO: color = t.solo; break;
            case ZT_STYLE_DIM: color = t.dim; break;
            default: break;
            }
            lv_obj_set_style_text_font(o.label, style == ZT_STYLE_PLAYING ? bold_font_ : font_, 0);
            lv_obj_set_style_text_color(o.label, lv_color_hex(color), 0);
            lv_obj_set_style_bg_color(o.label, lv_color_hex(bg), 0);
            lv_obj_set_pos(o.label, x, y);
            lv_obj_set_size(o.label, width * cell_w_, cell_h_);
            lv_label_set_text(o.label, text.c_str());
            lv_obj_remove_flag(o.label, LV_OBJ_FLAG_HIDDEN);
            o = Overlay{o.label, text, x, y, style, bg, true};
        }
    }
}

// ---- Sample editor ---------------------------------------------------

namespace {
// The processes, in the order of the PROCESS menu (most used first).
struct SampleProcess {
    const char* name;
    int op;
};
constexpr std::array<SampleProcess, 20> kSampleProcesses{{
    {"NORMALIZE", ZT_SAMPLE_NORMALIZE},         {"CROP", ZT_SAMPLE_CROP},
    {"DELETE", ZT_SAMPLE_DELETE},               {"DUPLICATE", ZT_SAMPLE_DUPLICATE},
    {"FADE IN", ZT_SAMPLE_FADE_IN},             {"FADE OUT", ZT_SAMPLE_FADE_OUT},
    {"REVERSE", ZT_SAMPLE_REVERSE},             {"SILENCE", ZT_SAMPLE_SILENCE},
    {"INVERT", ZT_SAMPLE_INVERT},               {"XFADE LOOP", ZT_SAMPLE_XFADE_LOOP},
    {"SLICE EVEN", ZT_SAMPLE_SLICE_EVEN},       {"SLICE AUTO", ZT_SAMPLE_SLICE_AUTO},
    {"SLICE SILENCE", ZT_SAMPLE_SLICE_SILENCE}, {"CLEAR SLICES", ZT_SAMPLE_CLEAR_SLICES},
    {"MONO MIX", ZT_SAMPLE_MONO_MIX},           {"MONO LEFT", ZT_SAMPLE_MONO_LEFT},
    {"MONO RIGHT", ZT_SAMPLE_MONO_RIGHT},       {"DOWNSAMPLE", ZT_SAMPLE_DOWNSAMPLE},
    {"16-BIT", ZT_SAMPLE_BITS_16},              {"8-BIT", ZT_SAMPLE_BITS_8},
}};

std::string seconds(uint32_t frames, uint32_t rate) {
    char s[16];
    std::snprintf(s, sizeof s, "%.3fs", rate ? double(frames) / rate : 0.0);
    return s;
}
}  // namespace

void TrackerScreen::open_sample_editor(const std::string& path) {
    char err[128] = {};
    ZtSampleEdit* edit = zt_sample_open(path.c_str(), err, sizeof err);
    if (!edit) {
        volume_text_ = std::string("SAMPLE: ") + err;
        volume_shown_at_ = lv_tick_get();
        return;
    }
    if (viewer_) zt_viewer_stop(viewer_);
    sample_edit_ = edit;
    sample_path_ = path;
    se_slice_ = 0;
    se_slice_edit_ = false;
    ZtSampleInfo info{};
    zt_sample_info(edit, &info);
    se_mode_ = 0;
    se_active_ = 1;
    se_from_ = 0;
    se_to_ = info.frames;
    se_menu_ = se_naming_ = se_confirm_exit_ = se_playing_ = false;
    se_message_.clear();
}

void TrackerScreen::close_sample_editor() {
    zt_sample_preview_stop(engine_);
    zt_sample_close(sample_edit_);
    sample_edit_ = nullptr;
    se_menu_ = se_naming_ = se_confirm_exit_ = se_playing_ = false;
    sample_view_.set_visible(false);
}

// ---- EQ editor -------------------------------------------------------------

namespace {
const char* const kEqTypes[] = {"LOWCUT", "LOWSHELF", "BELL", "BANDPASS", "HI.SHELF", "HI.CUT", "ALLPASS"};
const char* const kEqModes[] = {"STEREO", "MID", "SIDE", "LEFT", "RIGHT"};
const char* const kEqBands[] = {"LOW", "MID", "HIGH"};
}  // namespace

void TrackerScreen::open_eq_editor() {
    if (!viewer_) return;
    ZtEqState st{};
    zt_viewer_eq_state(viewer_, &st);
    if (st.index < 0) return;
    eq_open_ = true;
    eq_changed_ = true;
    eq_band_ = 1;
}

void TrackerScreen::close_eq_editor() {
    eq_open_ = false;
    eq_view_.set_visible(false);
}

bool TrackerScreen::eq_editor_input(uint32_t pressed, uint32_t held, bool enter, bool escape,
                                    bool del, uint32_t soft, const std::string& typed, bool ctrl) {
    const uint32_t now = lv_tick_get();
    // Arrows held: repeat after 300 ms every 40 ms, coarse steps after 1 s.
    const uint32_t arrows = ZT_BTN_UP | ZT_BTN_DOWN | ZT_BTN_LEFT | ZT_BTN_RIGHT;
    uint32_t step = pressed & arrows;
    bool coarse = false;
    if (step) {
        eq_hold_since_ = eq_hold_last_ = now;
    }
    else if ((held & arrows) && now - eq_hold_since_ > 300 && now - eq_hold_last_ >= 40) {
        eq_hold_last_ = now;
        step = held & arrows;
        coarse = now - eq_hold_since_ > 1000;
    }
    if (!step && !enter && !escape && !del && !soft && typed.empty() && !(pressed & ZT_BTN_PLAY)) return false;
    eq_changed_ = true;
    if (escape) {
        close_eq_editor();
        return true;
    }
    if (ctrl) {
        for (char ch : typed) {
            if (ch == 'Z' || ch == 'Y') zt_viewer_undo(viewer_, ch == 'Y' ? 1 : 0);
        }
        return true;
    }
    // 1 2 3 pick a band, ENTER the next one; - and = (or [ ]) change Q.
    for (char ch : typed) {
        if (ch >= '1' && ch <= '3') eq_band_ = ch - '1';
        else if (ch == '-' || ch == '[') zt_viewer_eq_adjust(viewer_, eq_band_, ZT_EQ_Q, -1, 0);
        else if (ch == '=' || ch == ']' || ch == '+') zt_viewer_eq_adjust(viewer_, eq_band_, ZT_EQ_Q, 1, 0);
    }
    if (enter) eq_band_ = (eq_band_ + 1) % 3;
    if (step & ZT_BTN_UP) zt_viewer_eq_adjust(viewer_, eq_band_, ZT_EQ_GAIN, 1, coarse);
    if (step & ZT_BTN_DOWN) zt_viewer_eq_adjust(viewer_, eq_band_, ZT_EQ_GAIN, -1, coarse);
    if (step & ZT_BTN_RIGHT) zt_viewer_eq_adjust(viewer_, eq_band_, ZT_EQ_FREQ, 1, coarse);
    if (step & ZT_BTN_LEFT) zt_viewer_eq_adjust(viewer_, eq_band_, ZT_EQ_FREQ, -1, coarse);
    if (del || (soft & 4u)) zt_viewer_eq_reset(viewer_, eq_band_);
    if (soft & 1u) zt_viewer_eq_adjust(viewer_, eq_band_, ZT_EQ_TYPE, 1, 0);
    if (soft & 2u) zt_viewer_eq_adjust(viewer_, eq_band_, ZT_EQ_MODE, 1, 0);
    // PLAY: the instrument alone while the song is stopped (again: stop).
    if ((soft & 8u) || (pressed & ZT_BTN_PLAY)) {
        if (instrument_preview_) end_instrument_preview();
        else {
            zt_viewer_input(viewer_, ZT_BTN_PLAY, ZT_BTN_PLAY);
            after_input();
        }
    }
    return true;
}

bool TrackerScreen::draw_eq_editor() {
    const bool show = eq_open_ && viewer_;
    eq_view_.set_visible(show);
    if (!show) return false;
    ZtEqState st{};
    zt_viewer_eq_state(viewer_, &st);
    if (st.index < 0) {  // an undo took the EQ away
        close_eq_editor();
        return false;
    }
    // While it plays only the spectrum moves: about 16 frames a second, a
    // key at once.
    const uint32_t now = lv_tick_get();
    if (!eq_changed_ && now - eq_drawn_at_ < 60) return true;
    eq_changed_ = false;
    eq_drawn_at_ = now;
    EqEditorView::State s;
    char title[48];
    std::snprintf(title, sizeof title, "EQ %02X", static_cast<unsigned>(st.index));
    s.title = title;
    s.selected = eq_band_;
    const size_t n = static_cast<size_t>(std::max(2, eq_view_.columns()));
    s.total.resize(n);
    zt_eq_curve(&st, -1, s.total.data(), n);
    for (int b = 0; b < 3; ++b) {
        const ZtEqBand& z = st.bands[b];
        EqEditorView::Band& v = s.bands[static_cast<size_t>(b)];
        v.name = kEqBands[b];
        v.kind = z.kind < 7 ? kEqTypes[z.kind] : "?";
        v.mode = z.mode < 5 ? kEqModes[z.mode] : "?";
        v.freq = z.freq;
        v.gain = z.gain / 100.0;
        v.q = z.q;
        v.has_gain = z.kind == 1 || z.kind == 2 || z.kind == 4;
        s.band_curve[static_cast<size_t>(b)].resize(n);
        zt_eq_curve(&st, b, s.band_curve[static_cast<size_t>(b)].data(), n);
    }
    // The spectrum of the instrument being equalized, not the master
    //: the sum of the tracks playing it.
    if (!eq_spectrum_) {
        eq_spectrum_ = std::make_unique<Spectrum>(ZT_TRACK_OUTPUT_WINDOW_FRAMES, 1, 44100.0f, 20.0f, 20000.0f);
        eq_tap_.resize(eq_spectrum_->size());
    }
    bool fresh = false;
    if (engine_ && (zt_viewer_playing(viewer_) || instrument_preview_)) {
        // A preview plays only this instrument, on the preview's own song.
        const uint8_t tracks = instrument_preview_ ? 0xFF : zt_viewer_eq_tracks(viewer_);
        std::fill(eq_tap_.begin(), eq_tap_.end(), 0.0f);
        std::vector<float> one(eq_tap_.size());
        for (uint8_t t = 0; t < 8; ++t) {
            if (!(tracks & (1u << t)) ||
                zt_engine_read_track_output(engine_, t, one.data(), one.size()) != one.size()) {
                continue;
            }
            for (size_t i = 0; i < one.size(); ++i) eq_tap_[i] += one[i];
            fresh = true;
        }
    }
    s.spectrum = eq_spectrum_->columns(fresh ? eq_tap_.data() : nullptr, n,
                                       static_cast<float>(EqEditorView::kMinHz),
                                       static_cast<float>(EqEditorView::kMaxHz), 0.05f);
    eq_view_.set(std::move(s));
    return true;
}

void TrackerScreen::se_say(const std::string& message) {
    se_message_ = message;
    se_message_at_ = lv_tick_get();
}

// The selected slice: from its marker to the next one (or the end); the
// whole sample when there are no slices.
std::pair<uint32_t, uint32_t> TrackerScreen::se_slice_bounds() const {
    ZtSampleInfo info{};
    zt_sample_info(sample_edit_, &info);
    std::vector<uint32_t> slices(info.slices);
    zt_sample_slices(sample_edit_, slices.data(), slices.size());
    if (slices.empty()) return {0, info.frames};
    const size_t i = static_cast<size_t>(std::clamp(se_slice_, 0, static_cast<int>(slices.size()) - 1));
    return {slices[i], i + 1 < slices.size() ? slices[i + 1] : info.frames};
}

// The marker the arrows move: an end of the selection or the loop, or the
// edge of the slice being edited.
uint32_t TrackerScreen::se_marker() const {
    ZtSampleInfo info{};
    zt_sample_info(sample_edit_, &info);
    if (se_mode_ == 2) {
        const auto [a, b] = se_slice_bounds();
        return se_slice_edit_ && se_slice_edge_ ? b : a;
    }
    if (se_mode_ == 1 && info.loop_start != 0xFFFFFFFFu) return se_active_ ? info.loop_end : info.loop_start;
    return se_active_ ? info.sel_end : info.sel_start;
}

void TrackerScreen::se_move_marker(int64_t delta) {
    ZtSampleInfo info{};
    zt_sample_info(sample_edit_, &info);
    const int64_t n = info.frames;
    const auto at = static_cast<uint32_t>(std::clamp<int64_t>(int64_t(se_marker()) + delta, 0, n));
    if (se_mode_ == 2) {
        // START is the slice's marker, END the next one's; the end of the
        // last slice is the end of the sample and does not move.
        const size_t idx = static_cast<size_t>(se_slice_) + static_cast<size_t>(se_slice_edge_);
        if (se_slice_edit_ && idx < info.slices) zt_sample_move_slice(sample_edit_, idx, at);
    }
    else if (se_mode_ == 1) {
        uint32_t a = info.loop_start, b = info.loop_end;
        if (a == 0xFFFFFFFFu) {
            a = info.sel_start;
            b = info.sel_end;
        }
        if (se_active_) b = std::max(at, a + 1);
        else a = std::min(at, b > 0 ? b - 1 : 0);
        zt_sample_set_loop(sample_edit_, a, b);
    }
    else {
        uint32_t a = info.sel_start, b = info.sel_end;
        if (se_active_) b = std::max(at, a);
        else a = std::min(at, b);
        zt_sample_set_selection(sample_edit_, a, b);
    }
    // Keep the marker on screen.
    const uint32_t m = se_marker(), span = se_to_ - se_from_;
    if (m < se_from_) {
        se_from_ = m;
        se_to_ = m + span;
    }
    else if (m > se_to_) {
        se_to_ = std::min<uint32_t>(m, info.frames);
        se_from_ = se_to_ - span;
    }
}

void TrackerScreen::se_play() {
    if (se_playing_) {
        zt_sample_preview_stop(engine_);
        se_playing_ = false;
        return;
    }
    ZtSampleInfo info{};
    zt_sample_info(sample_edit_, &info);
    uint32_t a = info.sel_start, b = info.sel_end;
    if (se_mode_ == 1 && info.loop_start != 0xFFFFFFFFu) {
        a = info.loop_start;
        b = info.loop_end;
    }
    else if (se_mode_ == 2) {
        std::tie(a, b) = se_slice_bounds();
    }
    if (a >= b) {
        a = 0;
        b = info.frames;
    }
    if (zt_sample_preview(sample_edit_, engine_, a, b)) {
        se_playing_ = true;
        se_play_from_ = a;
        se_play_to_ = b;
        se_play_tick_ = lv_tick_get();
    }
}

// Saves under se_name_: the same name overwrites the file, another one
// writes NAME.wav next to it and points the instrument at it.
void TrackerScreen::se_save() {
    namespace fs = std::filesystem;
    const fs::path old_path = sample_path_;
    std::string name = se_name_;
    while (!name.empty() && name.back() == ' ') name.pop_back();
    if (name.empty()) name = old_path.stem().string();
    const bool same = name == old_path.stem().string();
    const fs::path target = same ? old_path : old_path.parent_path() / (name + ".wav");
    char out[512] = {};
    if (!zt_sample_save(sample_edit_, same ? nullptr : target.c_str(), out, sizeof out)) {
        se_say(out);
        return;
    }
    sample_path_ = out;
    if (viewer_) {
        if (same) zt_viewer_reload_samples(viewer_);
        else zt_viewer_set_sample(viewer_, out);
    }
    se_say(tr({"SAVED", "SALVATO", "ENREGISTRÉ", "GESPEICHERT", "GUARDADO", "保存済", "已保存"}));
}

bool TrackerScreen::sample_editor_input(uint32_t pressed, uint32_t held, bool enter, bool escape,
                                        bool del, uint32_t soft, const std::string& typed, bool ctrl) {
    const int n_proc = static_cast<int>(kSampleProcesses.size());
    const uint32_t now = lv_tick_get();
    // LEFT/RIGHT held repeat: after 300 ms every 30 ms, the step growing
    // with the time held (up to 32 times).
    int repeat = 0;
    const uint32_t lr = held & (ZT_BTN_LEFT | ZT_BTN_RIGHT);
    if (pressed & (ZT_BTN_LEFT | ZT_BTN_RIGHT)) {
        se_hold_since_ = se_hold_last_ = now;
    }
    else if (lr && !se_naming_ && !se_confirm_exit_ && now - se_hold_since_ > 300 &&
             now - se_hold_last_ >= 30) {
        se_hold_last_ = now;
        repeat = std::min<int>(32, 1 + static_cast<int>((now - se_hold_since_) / 400));
    }
    if (!pressed && !repeat && !enter && !escape && !del && !soft && typed.empty()) return false;
    // The name for SAVE, typed into the modal.
    if (se_naming_) {
        if (escape || (soft & 8u)) {
            se_naming_ = false;
        }
        else if (enter || (soft & 1u)) {
            se_naming_ = false;
            se_save();
        }
        else if (del) {
            if (!se_name_.empty()) se_name_.pop_back();
        }
        else {
            for (char ch : typed) {
                if (se_name_.size() < 24) se_name_ += ch == ' ' ? '_' : ch;
            }
        }
        return true;
    }
    // Leaving with unsaved changes: ENTER saves, DEL discards, ESC stays.
    if (se_confirm_exit_) {
        if (enter || (soft & 1u)) {
            se_confirm_exit_ = false;
            se_name_ = std::filesystem::path(sample_path_).stem().string();
            se_save();
            close_sample_editor();
        }
        else if (del || (soft & 2u)) {
            close_sample_editor();
        }
        else if (escape || (soft & 8u)) {
            se_confirm_exit_ = false;
        }
        return true;
    }
    // PROCESS menu: UP/DOWN pick, LEFT/RIGHT the slice count, ENTER runs.
    if (se_menu_) {
        if (pressed & ZT_BTN_UP) se_menu_index_ = (se_menu_index_ + n_proc - 1) % n_proc;
        if (pressed & ZT_BTN_DOWN) se_menu_index_ = (se_menu_index_ + 1) % n_proc;
        if (kSampleProcesses[se_menu_index_].op == ZT_SAMPLE_SLICE_EVEN) {
            // Any count 2-128: LEFT/RIGHT by one (held: repeating), L held by 8.
            const int d = (held & ZT_BTN_EDIT) ? 8 : 1;
            const bool more = (pressed & ZT_BTN_RIGHT) || (repeat && (held & ZT_BTN_RIGHT));
            const bool less = (pressed & ZT_BTN_LEFT) || (repeat && (held & ZT_BTN_LEFT));
            if (more) se_slice_parts_ = std::min(128, se_slice_parts_ + d);
            if (less) se_slice_parts_ = std::max(2, se_slice_parts_ - d);
        }
        if (escape || (soft & 8u)) {
            se_menu_ = false;
        }
        else if (enter || (soft & 1u)) {
            char err[64] = {};
            const auto& p = kSampleProcesses[se_menu_index_];
            se_menu_ = false;
            if (zt_sample_process(sample_edit_, p.op, se_slice_parts_, err, sizeof err)) {
                se_say(p.name);
                // After slicing, SLICE mode: the slices can be adjusted one by one.
                if (p.op == ZT_SAMPLE_SLICE_EVEN || p.op == ZT_SAMPLE_SLICE_AUTO ||
                    p.op == ZT_SAMPLE_SLICE_SILENCE) {
                    se_mode_ = 2;
                    se_slice_ = 0;
                    se_slice_edit_ = false;
                }
            }
            else {
                se_say(err);
            }
        }
        return true;
    }
    if (ctrl) {
        for (char ch : typed) {
            if (ch == 'Z' || ch == 'Y') zt_sample_undo(sample_edit_, ch == 'Y' ? 1 : 0);
        }
        se_slice_edit_ = false;
        return true;
    }
    if (escape) {
        if (se_mode_ == 2 && se_slice_edit_) {
            se_slice_edit_ = false;  // back to choosing a slice
            return true;
        }
        ZtSampleInfo info{};
        zt_sample_info(sample_edit_, &info);
        if (info.dirty) se_confirm_exit_ = true;
        else close_sample_editor();
        return true;
    }
    ZtSampleInfo info{};
    zt_sample_info(sample_edit_, &info);
    if (soft & 1u) {
        se_slice_edit_ = false;
        se_mode_ = (se_mode_ + 1) % 3;
        se_active_ = 1;
        // LOOP on a sample without one starts from the selection.
        if (se_mode_ == 1 && info.loop_start == 0xFFFFFFFFu) {
            const bool sel = info.sel_start < info.sel_end;
            zt_sample_set_loop(sample_edit_, sel ? info.sel_start : 0, sel ? info.sel_end : info.frames);
        }
    }
    if (soft & 2u) se_menu_ = true;
    if ((soft & 4u) || (pressed & ZT_BTN_PLAY)) se_play();
    if (soft & 8u) {
        se_naming_ = true;
        se_name_ = std::filesystem::path(sample_path_).stem().string();
    }
    const int cols = std::max(1, sample_view_.columns());
    const int64_t per_col = std::max<int64_t>(1, (int64_t(se_to_) - se_from_) / cols);
    const int64_t step = ((held & ZT_BTN_EDIT) ? per_col * 16 : per_col) * std::max(1, repeat);
    const bool left = (pressed & ZT_BTN_LEFT) || (repeat && (held & ZT_BTN_LEFT));
    const bool right = (pressed & ZT_BTN_RIGHT) || (repeat && (held & ZT_BTN_RIGHT));
    if (se_mode_ == 2 && !se_slice_edit_) {
        // Choosing: LEFT/RIGHT go from slice to slice, the view follows.
        const int last = std::max(0, static_cast<int>(info.slices) - 1);
        if (left) se_slice_ = std::max(0, se_slice_ - 1);
        if (right) se_slice_ = std::min(last, se_slice_ + 1);
        const auto [a, b] = se_slice_bounds();
        const uint32_t span = se_to_ - se_from_;
        if (a < se_from_ || b > se_to_) {
            const uint32_t mid = a / 2 + b / 2;
            se_from_ = mid > span / 2 ? std::min(mid - span / 2, info.frames - span) : 0;
            se_to_ = se_from_ + span;
        }
    }
    else {
        if (left) se_move_marker(-step);
        if (right) se_move_marker(step);
    }
    // S splits the chosen slice in two (on a sample without slices: in half).
    if (se_mode_ == 2 && !se_slice_edit_ && typed.find('S') != std::string::npos) {
        if (info.slices == 0) {
            zt_sample_add_slice(sample_edit_, 0);
            zt_sample_add_slice(sample_edit_, info.frames / 2);
        }
        else {
            const auto [a, b] = se_slice_bounds();
            if (b > a + 1) zt_sample_add_slice(sample_edit_, a + (b - a) / 2);
        }
    }
    // Zoom about the marker: UP in, DOWN out.
    if (pressed & (ZT_BTN_UP | ZT_BTN_DOWN)) {
        const int64_t span = int64_t(se_to_) - se_from_;
        int64_t next = (pressed & ZT_BTN_UP) ? span / 2 : span * 2;
        next = std::clamp<int64_t>(next, std::min<int64_t>(cols, info.frames), info.frames);
        const int64_t m = se_marker();
        const int64_t from = std::clamp<int64_t>(m - next / 2, 0, int64_t(info.frames) - next);
        se_from_ = static_cast<uint32_t>(from);
        se_to_ = static_cast<uint32_t>(from + next);
    }
    if (enter) {
        if (se_mode_ != 2) {
            se_active_ = 1 - se_active_;
        }
        else if (se_slice_edit_) {
            se_slice_edge_ = 1 - se_slice_edge_;  // START <-> END
        }
        else if (info.slices > 0) {
            zt_sample_begin_edit(sample_edit_);  // one undo step for the edit
            se_slice_edit_ = true;
            se_slice_edge_ = 0;
        }
    }
    if (del) {
        if (se_mode_ == 2) {
            // Choosing: the slice's marker goes, it joins the one before.
            if (!se_slice_edit_ && info.slices > 0) {
                zt_sample_remove_slice(sample_edit_, static_cast<size_t>(se_slice_));
                se_slice_ = std::max(0, se_slice_ - 1);
            }
        }
        else if (se_mode_ == 1) {
            zt_sample_set_loop(sample_edit_, 0, 0);
        }
        else {
            zt_sample_set_selection(sample_edit_, 0, info.frames);
        }
    }
    return true;
}

bool TrackerScreen::draw_sample_editor() {
    const bool show = sample_edit_ != nullptr;
    sample_view_.set_visible(show);
    if (!show) return false;
    ZtSampleInfo info{};
    zt_sample_info(sample_edit_, &info);
    if (se_to_ > info.frames || se_to_ <= se_from_) {
        se_from_ = 0;
        se_to_ = info.frames;
    }
    SampleEditorView::State st;
    st.title = std::filesystem::path(sample_path_).filename().string() + (info.dirty ? "*" : "");
    char fmt[64];
    std::snprintf(fmt, sizeof fmt, "%.1fk  %s  %u bit  %s", info.rate / 1000.0,
                  info.channels == 2 ? "stereo" : "mono", unsigned(info.bits),
                  seconds(info.frames, info.rate).c_str());
    st.format = fmt;
    static const char* modes_en[3] = {"SELECT", "LOOP", "SLICE"};
    static const char* modes_it[3] = {"SELEZ.", "LOOP", "SLICE"};
    st.mode = settings_.language == 1 ? modes_it[se_mode_] : modes_en[se_mode_];
    st.frames = info.frames;
    st.view_from = se_from_;
    st.view_to = se_to_;
    const int cols = sample_view_.columns();
    st.lo.resize(static_cast<size_t>(cols));
    st.hi.resize(static_cast<size_t>(cols));
    zt_sample_peaks(sample_edit_, se_from_, se_to_, static_cast<size_t>(cols), st.lo.data(), st.hi.data());
    st.sel_a = info.sel_start;
    st.sel_b = info.sel_end;
    if (se_mode_ == 2) std::tie(st.sel_a, st.sel_b) = se_slice_bounds();  // the chosen slice
    st.has_loop = info.loop_start != 0xFFFFFFFFu;
    st.loop_a = info.loop_start;
    st.loop_b = info.loop_end;
    st.slices.resize(info.slices);
    zt_sample_slices(sample_edit_, st.slices.data(), st.slices.size());
    st.marker = se_marker();
    if (!se_message_.empty() && lv_tick_get() - se_message_at_ < 2500) {
        st.footer = se_message_;
    }
    else if (se_mode_ == 2) {
        const auto [a, b] = se_slice_bounds();
        char f[128];
        if (info.slices == 0) {
            std::snprintf(f, sizeof f, "%s", tr({"no slices   S splits   PROCESS: SLICE", "nessuna slice   S divide   AZIONI: SLICE", "aucune slice   S coupe   PROCESS: SLICE", "keine Slices   S teilt   PROCESS: SLICE", "sin slices   S divide   PROCESS: SLICE", "スライスなし   S 分割   PROCESS: SLICE", "无切片   S 分割   PROCESS: SLICE"}));
        }
        else if (se_slice_edit_) {
            const std::string sa = seconds(a, info.rate), sb = seconds(b, info.rate);
            std::snprintf(f, sizeof f, "%02X %s  %s   ENTER %s  ESC ok", se_slice_,
                          se_slice_edge_ ? (" " + sa).c_str() : ("[" + sa + "]").c_str(),
                          se_slice_edge_ ? ("[" + sb + "]").c_str() : (sb + " ").c_str(),
                          tr({"start/end", "inizio/fine", "début/fin", "Anfang/Ende", "inicio/fin", "開始/終了", "起点/终点"}));
        }
        else {
            std::snprintf(f, sizeof f, "SLICE %02X/%02X  %s   ENTER %s  S  DEL", se_slice_,
                          unsigned(info.slices) - 1, seconds(b - a, info.rate).c_str(), tr({"edit", "modifica", "modifier", "bearbeiten", "editar", "編集", "编辑"}));
        }
        st.footer = f;
    }
    else {
        const uint32_t a = se_mode_ == 1 && st.has_loop ? info.loop_start : info.sel_start;
        const uint32_t b = se_mode_ == 1 && st.has_loop ? info.loop_end : info.sel_end;
        const std::string sa = seconds(a, info.rate), sb = seconds(b, info.rate);
        st.footer = (se_active_ ? " " + sa + "  [" + sb + "]" : "[" + sa + "]  " + sb + " ") + "  LEN " +
                    seconds(b > a ? b - a : 0, info.rate);
    }
    sample_view_.set(std::move(st));
    clear_grid();
    return true;
}

// The question modal: shown while a question waits for ENTER / DEL / ESC.
void TrackerScreen::update_modal() {
    if (!modal_) return;
    std::string title, body, keys;
    int bar = -1;  // RENDER's or LIBRARY's progress, 0..1000, or -1: no bar
    const char* any_key = tr({"any key closes", "un tasto chiude", "une touche ferme", "eine Taste schließt", "una tecla cierra", "任意のキーで閉じる", "任意键关闭"});
    ZtPackInfo pack{};
    const bool have_pack = library_open_ && library_pack(lib_selected_, pack);
    if (library_open_ && lib_modal_) {
        uint32_t permille = 0;
        char msg[160] = {};
        const uint8_t state = library_ ? zt_library_status(library_, &permille, msg, sizeof msg) : ZT_LIB_FAILED;
        body = lib_job_;
        if (state == ZT_LIB_DOWNLOADING || state == ZT_LIB_INSTALLING) {
            title = state == ZT_LIB_DOWNLOADING
                ? tr({"Downloading", "Download in corso", "Téléchargement", "Herunterladen", "Descargando", "ダウンロード中", "下载中"})
                : tr({"Installing", "Installazione", "Installation", "Installieren", "Instalando", "インストール中", "安装中"});
            char pct[16];
            std::snprintf(pct, sizeof pct, "  %u%%", permille / 10);
            body += pct;
            keys = tr({"ESC cancel", "ESC annulla", "ESC annuler", "ESC abbrechen", "ESC cancelar", "ESC 中止", "ESC 取消"});
            bar = static_cast<int>(permille);
        }
        else {
            title = state == ZT_LIB_DONE ? tr({"Done", "Fatto", "Terminé", "Fertig", "Hecho", "完了", "完成"})
                    : state == ZT_LIB_CANCELLED ? tr({"Cancelled", "Annullato", "Annulé", "Abgebrochen", "Cancelado", "中止", "已取消"})
                                                : tr({"Failed", "Non riuscito", "Échec", "Fehlgeschlagen", "Fallido", "失敗", "失败"});
            // The first clause of the library's message ("Removed 24 files; ...").
            if (msg[0]) body = std::string(msg).substr(0, std::string(msg).find(';'));
            keys = any_key;
        }
    }
    else if (library_open_ && lib_info_ && have_pack) {
        title = pack.name;
        body = std::string(pack.description) + "\n" + pack.authors + "\n" + pack.licence + ", " + pack_size(pack.bytes);
        keys = any_key;
    }
    else if (library_open_ && lib_confirm_remove_ && have_pack) {
        title = tr({"Remove the pack?", "Togliere il pacchetto?", "Supprimer le pack ?", "Paket entfernen?", "¿Quitar el paquete?", "パックを削除しますか？", "删除资源包？"});
        body = pack.name;
        keys = tr({"ENTER yes   ESC no", "ENTER si   ESC no", "ENTER oui   ESC non", "ENTER ja   ESC nein", "ENTER sí   ESC no", "ENTER はい   ESC いいえ", "ENTER 是   ESC 否"});
    }
    else if (render_choice_ >= 0) {
        title = tr({"Render", "Render", "Rendu", "Rendern", "Render", "レンダー", "渲染"});
        const char* what[3] = {
            tr({"MIX", "MIX", "MIX", "MIX", "MEZCLA", "ミックス", "混音"}),
            tr({"STEMS", "STEM", "STEMS", "STEMS", "STEMS", "ステム", "分轨"}),
            tr({"MIX + STEMS", "MIX + STEM", "MIX + STEMS", "MIX + STEMS", "MEZCLA + STEMS", "ミックス + ステム", "混音 + 分轨"})};
        body = std::string("< ") + what[render_choice_] + " >";
        keys = tr({"</> choose  ENTER start  ESC cancel", "</> scegli  ENTER avvia  ESC annulla", "</> choix  ENTER lancer  ESC annuler", "</> wählen  ENTER start  ESC zurück", "</> elegir  ENTER iniciar  ESC salir", "</> 選択  ENTER 開始  ESC 中止", "</> 选择  ENTER 开始  ESC 取消"});
    }
    else if (render_modal_) {
        uint8_t done = 0, passes = 0;
        uint32_t permille = 0;
        const uint8_t state = render_ ? zt_render_status(render_, &done, &passes, &permille) : ZT_RENDER_FAILED;
        char header[64] = {};
        if (viewer_) zt_viewer_header(viewer_, header, sizeof header);
        std::string name = header;
        if (const auto at = name.find("  BPM "); at != std::string::npos) name.resize(at);
        if (!name.empty() && name.back() == '*') name.pop_back();
        if (state == ZT_RENDER_RUNNING) {
            title = tr({"Rendering", "Render in corso", "Rendu en cours", "Rendern läuft", "Renderizando", "レンダー中", "渲染中"});
            char pct[16];
            std::snprintf(pct, sizeof pct, "  %u%%", permille / 10);
            body = name + pct;
            keys = tr({"ESC cancel", "ESC annulla", "ESC annuler", "ESC abbrechen", "ESC cancelar", "ESC 中止", "ESC 取消"});
            bar = static_cast<int>(permille);
        }
        else if (state == ZT_RENDER_DONE) {
            title = tr({"Render done", "Render completato", "Rendu terminé", "Rendern fertig", "Render terminado", "レンダー完了", "渲染完成"});
            body = "Renders/" + name + "/";
            keys = tr({"any key closes", "un tasto chiude", "une touche ferme", "eine Taste schließt", "una tecla cierra", "任意のキーで閉じる", "任意键关闭"});
        }
        else {
            title = tr({"Render failed", "Render non riuscito", "Rendu échoué", "Rendern fehlgeschlagen", "Render fallido", "レンダー失敗", "渲染失败"});
            body = name;
            keys = tr({"any key closes", "un tasto chiude", "une touche ferme", "eine Taste schließt", "una tecla cierra", "任意のキーで閉じる", "任意键关闭"});
        }
    }
    else if (sample_edit_ && se_menu_) {
        const auto& p = kSampleProcesses[se_menu_index_];
        title = tr({"Process", "Azione sul sample", "Traitement", "Bearbeiten", "Procesar", "処理", "处理"});
        body = std::string("< ") + p.name +
               (p.op == ZT_SAMPLE_SLICE_EVEN ? " / " + std::to_string(se_slice_parts_) : std::string()) + " >";
        keys = tr({"UP/DOWN pick   ENTER apply   ESC close", "SU/GIU scegli   ENTER applica   ESC chiudi", "HAUT/BAS choisir   ENTER appliquer   ESC fermer", "AUF/AB wählen   ENTER anwenden   ESC schließen", "ARRIBA/ABAJO elegir   ENTER aplicar   ESC cerrar", "上下 選択   ENTER 適用   ESC 閉じる", "上下 选择   ENTER 应用   ESC 关闭"});
    }
    else if (sample_edit_ && se_naming_) {
        title = tr({"Save the sample", "Salva il sample", "Enregistrer le sample", "Sample speichern", "Guardar el sample", "サンプルを保存", "保存采样"});
        body = se_name_ + "_.wav";
        keys = tr({"ENTER save   ESC cancel", "ENTER salva   ESC annulla", "ENTER enregistrer   ESC annuler", "ENTER speichern   ESC abbrechen", "ENTER guardar   ESC cancelar", "ENTER 保存   ESC キャンセル", "ENTER 保存   ESC 取消"});
    }
    else if (sample_edit_ && se_confirm_exit_) {
        title = tr({"Save the sample?", "Salvare il sample?", "Enregistrer le sample ?", "Sample speichern?", "¿Guardar el sample?", "サンプルを保存しますか？", "保存采样？"});
        body = std::filesystem::path(sample_path_).filename().string();
        keys = tr({"ENTER save   DEL discard   ESC stay", "ENTER salva   DEL scarta   ESC resta", "ENTER enregistrer   DEL abandonner   ESC rester", "ENTER speichern   DEL verwerfen   ESC bleiben", "ENTER guardar   DEL descartar   ESC quedarse", "ENTER 保存   DEL 破棄   ESC 戻る", "ENTER 保存   DEL 放弃   ESC 留下"});
    }
    else if (viewer_ && confirm_close_) {
        char header[64] = {};
        zt_viewer_header(viewer_, header, sizeof header);
        std::string name = header;
        if (const auto at = name.find("  BPM "); at != std::string::npos) name.resize(at);
        if (!name.empty() && name.back() == '*') name.pop_back();  // the unsaved mark
        title = tr({"Save changes?", "Salvare le modifiche?", "Enregistrer les modifications ?", "Änderungen speichern?", "¿Guardar los cambios?", "変更を保存しますか？", "保存更改？"});
        body = name;
        keys = tr({"ENTER save  DEL drop  ESC cancel", "ENTER salva  DEL scarta  ESC annulla", "ENTER enreg.  DEL jeter  ESC annuler", "ENTER sichern  DEL weg  ESC zurück", "ENTER guardar  DEL tirar  ESC cancelar", "ENTER 保存  DEL 破棄  ESC 取消", "ENTER 保存  DEL 放弃  ESC 取消"});
    }
    else if (viewer_ && (zt_viewer_ux_state(viewer_) & ZT_UX_CONFIRM_OVERWRITE)) {
        char file[128] = {};
        zt_viewer_overwrite_name(viewer_, file, sizeof file);
        title = tr({"Overwrite the file?", "Sovrascrivere il file?", "Écraser le fichier ?", "Datei überschreiben?", "¿Sobrescribir el archivo?", "ファイルを上書きしますか？", "覆盖文件？"});
        body = file;
        keys = tr({"ENTER yes   ESC no", "ENTER si   ESC no", "ENTER oui   ESC non", "ENTER ja   ESC nein", "ENTER sí   ESC no", "ENTER はい   ESC いいえ", "ENTER 是   ESC 否"});
    }
    if (modal_bar_) {
        if (bar < 0) lv_obj_add_flag(modal_bar_, LV_OBJ_FLAG_HIDDEN);
        else {
            lv_obj_remove_flag(modal_bar_, LV_OBJ_FLAG_HIDDEN);
            lv_bar_set_value(modal_bar_, bar, LV_ANIM_OFF);
        }
    }
    const std::string key = title + "\x01" + body;
    if (key == modal_shown_) return;
    modal_shown_ = key;
    if (title.empty()) {
        lv_obj_add_flag(modal_, LV_OBJ_FLAG_HIDDEN);
        return;
    }
    // LIBRARY's INFO wraps its details over several lines; every other
    // question is one line.
    {
        const bool info = library_open_ && lib_info_ && !lib_modal_;
        const lv_font_t* body_font = info ? modal_keys_font_ : font_;
        const int line_h = lv_font_get_line_height(body_font);
        const int lines = info ? 5 : 1;
        lv_obj_set_style_text_font(modal_body_, body_font, 0);
        lv_label_set_long_mode(modal_body_, info ? LV_LABEL_LONG_MODE_WRAP : LV_LABEL_LONG_MODE_DOTS);
        lv_obj_set_height(modal_body_, lines * line_h);
        lv_obj_set_height(modal_, modal_base_h_ + lines * line_h - lv_font_get_line_height(font_));
        lv_obj_align(modal_, LV_ALIGN_CENTER, 0, -8);
    }
    lv_label_set_text(modal_title_, title.c_str());
    lv_label_set_text(modal_body_, body.c_str());
    lv_label_set_text(modal_keys_, keys.c_str());
    lv_obj_remove_flag(modal_, LV_OBJ_FLAG_HIDDEN);
    lv_obj_move_foreground(modal_);
}

void TrackerScreen::redraw() {
    if (page_ != kPageApp) app_view_.set_visible(false);
    // Edit mode shows in the cursor's colour, on every song view.
    {
        const Theme& t = theme();
        const bool editing = viewer_ && (zt_viewer_ux_state(viewer_) & ZT_UX_EDITING);
        chain_phrase_.set_editing(editing, t.alert);
        instrument_.set_editing(editing, t.alert);
        if (cursor_) lv_obj_set_style_bg_color(cursor_, lv_color_hex(editing ? t.alert : t.cursor), 0);
    }
    update_spectrum();
    update_modal();
    update_bar();
    update_meters();
    update_bottom_bar();
    if (manual_panel_) {
        if (manual_open_) {
            lv_obj_remove_flag(manual_panel_, LV_OBJ_FLAG_HIDDEN);
            lv_obj_move_foreground(manual_panel_);
        }
        else {
            lv_obj_add_flag(manual_panel_, LV_OBJ_FLAG_HIDDEN);
        }
    }
    if (draw_recorder()) {
        return;
    }
    if (draw_library()) {
        app_view_.set_visible(false);
        draw_home();
        draw_browser();
        draw_timeline();
        draw_mixer();
        draw_song();
        draw_tracks();
        draw_scopes();
        draw_chain_phrase();
        draw_instrument();
        draw_fx_picker();
        return;
    }
    if (draw_eq_editor() || draw_sample_editor()) {
        if (eq_open_) draw_sample_editor();
        draw_home();
        draw_browser();
        draw_timeline();
        draw_mixer();
        draw_song();
        draw_tracks();
        draw_scopes();
        draw_chain_phrase();
        draw_instrument();
        draw_fx_picker();
        return;
    }
    if (draw_home() | draw_browser()) {
        draw_timeline();
        draw_mixer();
        draw_song();
        draw_tracks();
        draw_scopes();
        draw_chain_phrase();
        draw_instrument();
        draw_fx_picker();
        return;
    }
    if (page_ == kPageApp) {
        // Hide the graphic pages: key 4 held reaches APP straight from TIME.
        draw_timeline();
        draw_mixer();
        draw_song();
        draw_tracks();
        draw_scopes();
        draw_chain_phrase();
        draw_instrument();
        draw_fx_picker();
        lv_obj_add_flag(selection_, LV_OBJ_FLAG_HIDDEN);
        draw_app();
        return;
    }
    if (draw_timeline() | draw_mixer() | draw_song() | draw_tracks() | draw_scopes() | draw_chain_phrase() |
        draw_instrument() | draw_fx_picker()) {
        return;
    }
    if (viewer_) {
        std::vector<char> buf(rows_count_ * (cols_ + 1));
        int32_t line = -1, col = -1, width = -1;
        const size_t n = zt_viewer_frame(viewer_, buf.data(), rows_count_, cols_, &line, &col, &width);
        std::vector<std::string> lines;
        for (size_t r = 0; r < n; ++r) {
            lines.emplace_back(&buf[r * (cols_ + 1)]);
        }
        if (line >= 0 && line < static_cast<int32_t>(lines.size())) {
            int w = width;
            show_text_entry(lines[line], col, w);
            width = w;
        }
        draw_lines(lines, line, col, width);
        draw_highlights(lines, line, col, width);
        int32_t sel[4] = {};
        if (zt_viewer_selection(viewer_, rows_count_, cols_, sel)) {
            lv_obj_set_pos(selection_, sel[2] * cell_w_, top_px_ + sel[0] * cell_h_);
            lv_obj_set_size(selection_, (sel[3] - sel[2]) * cell_w_, (sel[1] - sel[0] + 1) * cell_h_);
            lv_obj_remove_flag(selection_, LV_OBJ_FLAG_HIDDEN);
        }
        else {
            lv_obj_add_flag(selection_, LV_OBJ_FLAG_HIDDEN);
        }
        return;
    }
    lv_obj_add_flag(selection_, LV_OBJ_FLAG_HIDDEN);
}

// HOME without a song (page 0 before LOAD). False elsewhere.
bool TrackerScreen::draw_home() {
    const bool show = !viewer_ && page_ == 0 && !browsing_ && !library_open_;
    home_.set_visible(show);
    if (!show) return false;
    home_.set(home_items(), -1);  // no option highlighted
    clear_grid();
    return true;
}

// LOAD: the browser, after LOAD on HOME. False elsewhere.
bool TrackerScreen::draw_browser() {
    const bool show = browsing_ && page_ == 0 && !library_open_;
    browser_.set_visible(show);
    if (!show) return false;
    const int list_rows = browser_.visible_rows();
    if (selected_ < top_) top_ = selected_;
    if (selected_ >= top_ + list_rows) top_ = selected_ - list_rows + 1;
    std::vector<BrowserView::Row> rows;
    rows.reserve(entries_.size());
    for (const auto& e : entries_) rows.push_back({e.kind, e.name, e.detail, openable(e)});
    // Folder relative to the songs root, e.g. "/Songs/MINE".
    const std::string rel = dir_.size() > root_dir_.size() ? dir_.substr(root_dir_.size()) : "/";
    const std::string root_name = std::filesystem::path(root_dir_).filename().string();
    browser_.set(root_name + (rel == "/" ? "" : rel), rows, selected_, top_,
                 !browser_message_.empty() ? browser_message_ : error_);
    clear_grid();
    return true;
}

// TIMELINE in graphics; the text grid, cursor and overlays are cleared under
// it. False off the TIMELINE page (or in CHAIN, PHRASE... opened from it).
// Clears the text grid, cursor, overlays and selection under a graphic page.
void TrackerScreen::clear_grid() {
    draw_lines({}, -1, 0, 0);
    for (auto& o : overlays_) {
        if (o.visible) {
            lv_obj_add_flag(o.label, LV_OBJ_FLAG_HIDDEN);
            o.visible = false;
        }
    }
    lv_obj_add_flag(selection_, LV_OBJ_FLAG_HIDDEN);
}

bool TrackerScreen::draw_timeline() {
    static ZtTimeline view;
    const bool show = viewer_ && page_ == ZT_PAGE_TIMELINE &&
                      zt_viewer_timeline(viewer_, timeline_.visible_rows(), &view);
    timeline_.set_visible(show);
    if (!show) {
        return false;
    }
    timeline_.set(view);
    clear_grid();
    return true;
}

// Track levels for the graphic pages: L and R per track, then the master,
// from the engine; without an engine, the viewer's mono peaks (L = R).
static std::array<uint16_t, 18> stereo_levels(ZtViewer* viewer, ZtEngine* engine) {
    std::array<uint16_t, 18> out{};
    if (engine && zt_engine_track_peaks(engine, out.data())) {
        return out;
    }
    uint16_t mono[8] = {};
    if (viewer) zt_viewer_levels(viewer, mono);
    uint16_t master = 0;
    for (int t = 0; t < 8; ++t) {
        out[2 * t] = out[2 * t + 1] = mono[t];
        master = std::max(master, mono[t]);
    }
    out[16] = out[17] = master;
    return out;
}

// INSTRUMENT when the cursor is on an instrument on page 1.
bool TrackerScreen::draw_instrument() {
    static ZtInstrumentPage page;
    const bool show = viewer_ && page_ == ZT_PAGE_TIMELINE &&
                      zt_viewer_instrument_page(viewer_, static_cast<size_t>(rows_count_), &page);
    instrument_.set_visible(show);
    if (!show) {
        return false;
    }
    if (text_mode_ && page.cursor_line >= 0) {
        // The NAME being typed, in its own field.
        auto& raw = page.lines[page.cursor_line];
        std::string line(raw, strnlen(raw, sizeof raw));
        int width = page.cursor_width;
        show_text_entry(line, page.cursor_column, width);
        line.resize(std::min(line.size(), sizeof raw - 1));
        std::memcpy(raw, line.c_str(), line.size() + 1);
        for (int s = 0; s < page.span_count; ++s) {
            auto& sp = page.spans[s];
            if (sp.line == page.cursor_line && sp.column == page.cursor_column) sp.width = width;
        }
        page.cursor_width = width;
    }
    instrument_.set(page);
    clear_grid();
    return true;
}

// CHAIN | PHRASE when the cursor is in a chain or a phrase on page 1.
bool TrackerScreen::draw_chain_phrase() {
    static ZtChainPhrase view;
    const bool show = viewer_ && page_ == ZT_PAGE_TIMELINE && zt_viewer_chain_phrase(viewer_, &view);
    chain_phrase_.set_visible(show);
    if (!show) {
        return false;
    }
    chain_phrase_.set(view);
    clear_grid();
    return true;
}

void TrackerScreen::draw_app() {
    clear_grid();
    const Theme& t = theme();
    AppView::State s;
    s.labels = {tr({"FONT SIZE", "FONT", "POLICE", "SCHRIFT", "LETRA", "文字サイズ", "字号"}),
                tr({"THEME", "TEMA", "THÈME", "THEMA", "TEMA", "テーマ", "主题"}),
                tr({"SPECTRUM", "SPETTRO", "SPECTRE", "SPEKTRUM", "ESPECTRO", "スペクトル", "频谱"}),
                tr({"LANGUAGE", "LINGUA", "LANGUE", "SPRACHE", "IDIOMA", "言語", "语言"})};
    s.font_size = settings_.font_size;
    s.theme = kThemeNames[settings_.theme];
    for (size_t k = 0; k < s.swatches.size(); ++k) s.swatches[k] = t.chain[k];
    s.spectrum = settings_.spectrum;
    s.lang = lang();
    s.help_title = tr({"HOW TO USE", "ISTRUZIONI", "MODE D'EMPLOI", "ANLEITUNG", "CÓMO SE USA", "使い方", "使用方法"});
    for (const char* p : kHelpText[std::clamp(settings_.language, 0, zt::i18n::kCount - 1)]) s.help.push_back(p);
    s.selected = app_scroll_ > 0 ? -1 : app_field_;
    s.scroll = app_scroll_;
    s.version = "v0.1.0";
    app_view_.set(std::move(s));
    app_view_.set_visible(true);
}

// The FX command picker, on page 1 while it is open.
bool TrackerScreen::draw_fx_picker() {
    static ZtFxPicker p{};
    const bool show = viewer_ && page_ == ZT_PAGE_TIMELINE && zt_viewer_fx_picker(viewer_, &p);
    fx_picker_view_.set_visible(show);
    if (!show) {
        return false;
    }
    fx_picker_view_.set(p, tr({"FX COMMAND", "COMANDO FX", "COMMANDE FX", "FX-BEFEHL", "COMANDO FX", "FXコマンド",
                               "FX 命令"}));
    clear_grid();
    return true;
}

bool TrackerScreen::draw_tracks() {
    static ZtTracksPage p{};
    const bool show = viewer_ && page_ == ZT_PAGE_TRACKS && zt_viewer_tracks_page(viewer_, &p);
    tracks_view_.set_visible(show);
    if (!show) {
        return false;
    }
    // The cards show what each track plays even when MUTE or SOLO silences
    // it: the engine's levels before MUTE and SOLO.
    auto levels = stereo_levels(viewer_, engine_);
    if (engine_) zt_engine_track_activity(engine_, levels.data());
    tracks_view_.set(p, levels,
                     tr({"INSTRUMENT POOL", "STRUMENTI", "INSTRUMENTS", "INSTRUMENTE", "INSTRUMENTOS", "音色一覧", "乐器池"}),
                     "--");
    clear_grid();
    return true;
}

// RENDER as chosen in the modal: 0 the mix, 1 the stems, 2 both.
void TrackerScreen::start_render(int choice) {
    const uint8_t write = choice == 0 ? ZT_RENDER_MIX : choice == 1 ? ZT_RENDER_STEMS : ZT_RENDER_MIX | ZT_RENDER_STEMS;
    zt_render_free(render_);
    render_ = zt_render_start(viewer_, root_dir_.c_str(), write);
    render_modal_ = true;
    render_text_.clear();
    render_failed_ = render_ == nullptr;
    if (!render_) render_text_ = tr({"FAILED", "ERRORE", "ÉCHEC", "FEHLER", "ERROR", "失敗", "失败"});
}

// The RENDER modal's keys (live and scripted): the choice, then cancel or
// close. True when the key was the modal's.
bool TrackerScreen::render_modal_key(uint32_t pressed, bool enter, bool escape, uint32_t soft, bool any) {
    if (render_choice_ >= 0) {
        if (pressed & ZT_BTN_LEFT) render_choice_ = (render_choice_ + 2) % 3;
        else if (pressed & ZT_BTN_RIGHT) render_choice_ = (render_choice_ + 1) % 3;
        else if (enter || (soft & 1u)) {
            const int choice = render_choice_;
            render_choice_ = -1;
            start_render(choice);
        }
        else if (escape || (soft & 8u)) render_choice_ = -1;
        redraw();
        return true;
    }
    if (!render_modal_) return false;
    const bool running = render_ && zt_render_status(render_, nullptr, nullptr, nullptr) == ZT_RENDER_RUNNING;
    if (running && (escape || (soft & 8u))) {
        zt_render_free(render_);
        render_ = nullptr;
        render_text_ = tr({"STOP", "STOP", "STOP", "STOP", "PARADO", "中止", "已停止"});
        render_failed_ = true;
        render_modal_ = false;
    }
    else if (!running && any) {
        render_modal_ = false;
    }
    redraw();
    return true;
}

// RENDER's progress on the SONG page, one percentage over the mix and the
// stems, then DONE (kept until the next RENDER or another song).
void TrackerScreen::update_render() {
    if (!render_) return;
    uint8_t pass = 0, passes = 0;
    uint32_t permille = 0;
    const uint8_t state = zt_render_status(render_, &pass, &passes, &permille);
    std::string text;
    if (state == ZT_RENDER_RUNNING) {
        // One percentage over every pass (the mix, then the stems).
        char buf[16];
        std::snprintf(buf, sizeof buf, "%u%%", permille / 10);
        text = buf;
    }
    else if (state == ZT_RENDER_DONE) text = tr({"DONE", "FATTO", "FAIT", "FERTIG", "HECHO", "完了", "完成"});
    else if (state == ZT_RENDER_CANCELLED) text = tr({"STOP", "STOP", "STOP", "STOP", "PARADO", "中止", "已停止"});
    else text = tr({"FAILED", "ERRORE", "ÉCHEC", "FEHLER", "ERROR", "失敗", "失败"});
    const bool failed = state == ZT_RENDER_FAILED || state == ZT_RENDER_CANCELLED;
    if (text != render_text_ || failed != render_failed_) {
        render_text_ = text;
        render_failed_ = failed;
        if (page_ == ZT_PAGE_SONG || render_modal_) redraw();
    }
}

bool TrackerScreen::draw_song() {
    ZtSongPage p{};
    const bool show = viewer_ && page_ == ZT_PAGE_SONG && zt_viewer_song_page(viewer_, &p);
    song_view_.set_visible(show);
    if (!show) {
        return false;
    }
    const SongView::Words words{tr({"SAVE", "SALVA", "ENREG.", "SICHERN", "GUARDAR", "保存", "保存"}),       tr({"NEW", "NUOVA", "NOUVEAU", "NEU", "NUEVO", "新規", "新建"}),
                                tr({"LOAD", "CARICA", "CHARGER", "LADEN", "CARGAR", "読込", "载入"}),      tr({"UNSAVED", "NON SALVATA", "NON ENREGISTRÉ", "UNGESPEICHERT", "SIN GUARDAR", "未保存", "未保存"}),
                                tr({"ROWS", "RIGHE", "LIGNES", "ZEILEN", "FILAS", "行", "行"}),       tr({"CHAINS", "CHAIN", "CHAINS", "CHAINS", "CHAINS", "チェーン", "链"}),
                                tr({"PHRASES", "FRASI", "PHRASES", "PHRASEN", "FRASES", "フレーズ", "乐句"}),    tr({"INSTR.", "STRUM.", "INSTR.", "INSTR.", "INSTR.", "音色", "乐器"}),
                                tr({"RENDER", "RENDER", "RENDU", "RENDERN", "RENDER", "レンダー", "渲染"})};
    song_view_.set(p, words, text_mode_ && p.field == 0 ? text_ + "_" : std::string());
    song_view_.set_render_status(render_text_, render_failed_);
    clear_grid();
    return true;
}

bool TrackerScreen::draw_mixer() {
    ZtMixer m{};
    const bool show = viewer_ && page_ == ZT_PAGE_MIXER && zt_viewer_mixer(viewer_, &m);
    mixer_view_.set_visible(show);
    if (!show) {
        return false;
    }
    mixer_view_.set(m, stereo_levels(viewer_, engine_));
    clear_grid();
    mixer_bars_.set_visible(false);
    return true;
}

// OSCILLOSCOPES: the latest 512 samples of each track from the engine.
bool TrackerScreen::draw_scopes() {
    const bool show = viewer_ && page_ == kPageOscil;
    scopes_.set_visible(show);
    if (!show) {
        return false;
    }
    const bool playing = engine_ && zt_viewer_playing(viewer_);
    track_tap_.resize(512);
    for (int t = 0; t < 8; ++t) {
        const bool fresh = playing && zt_engine_read_track_output(engine_, static_cast<uint8_t>(t),
                                                                  track_tap_.data(),
                                                                  track_tap_.size()) == track_tap_.size();
        scopes_.set_track(t, fresh ? track_tap_.data() : nullptr, fresh ? track_tap_.size() : 0);
    }
    scopes_.tick();
    clear_grid();
    return true;
}

void TrackerScreen::draw_lines(const std::vector<std::string>& lines, int cursor_line,
                               int cursor_col, int cursor_width) {
    // Only rows whose text changed: every set_text redraws the row through
    // FreeType, which on the device cost ~90% of a core at 20 Hz.
    shown_.resize(rows_count_);
    for (int r = 0; r < rows_count_; ++r) {
        const std::string text = r < static_cast<int>(lines.size()) ? lines[r] : "";
        if (text != shown_[r]) {
            lv_label_set_text(rows_[r], text.c_str());
            shown_[r] = text;
        }
    }
    if (cursor_line >= 0 && cursor_line < rows_count_ && cursor_width > 0) {
        lv_obj_set_pos(cursor_, cursor_col * cell_w_, top_px_ + cursor_line * cell_h_);
        lv_obj_set_size(cursor_, cursor_width * cell_w_, cell_h_);
        lv_obj_remove_flag(cursor_, LV_OBJ_FLAG_HIDDEN);
    }
    else {
        lv_obj_add_flag(cursor_, LV_OBJ_FLAG_HIDDEN);
    }
}


// LIBRARY: the catalog opens (and is fetched again) each time; the handle
// lives until the app quits so a download goes on after the page closes.
void TrackerScreen::open_library() {
    if (!library_) {
        const char* env = std::getenv("ZT_LIBRARY_URL");
        const char* url = env && *env ? env : !settings_.library_url.empty() ? settings_.library_url.c_str() : kLibraryUrl;
        library_ = zt_library_open(root_dir_.c_str(), url);
    }
    if (library_) zt_library_refresh(library_);
    library_open_ = true;
    page_ = kPageLibrary;
    lib_selected_ = 0;
    lib_top_ = 0;
    lib_info_ = lib_confirm_remove_ = lib_modal_ = false;
    lib_shown_state_ = 0xFF;
}

void TrackerScreen::close_library() {
    library_open_ = false;
    lib_info_ = lib_confirm_remove_ = lib_modal_ = false;
    // New packs show in LOAD at once.
    if (browsing_) list_directory();
}

bool TrackerScreen::library_pack(int index, ZtPackInfo& out) const {
    return library_ && index >= 0 && zt_library_pack(library_, static_cast<uint32_t>(index), &out) != 0;
}

// GET on a pack not installed, REMOVE (after YES) on one that is.
void TrackerScreen::library_action(int slot) {
    ZtPackInfo info{};
    if (!library_pack(lib_selected_, info)) return;
    if (slot == 1) {
        if (info.installed) {
            lib_confirm_remove_ = true;
        }
        else if (zt_library_install(library_, static_cast<uint32_t>(lib_selected_))) {
            lib_job_ = info.name;
            lib_modal_ = true;
        }
        else {
            lib_job_ = info.name;
            lib_modal_ = true;  // shows why it did not start
        }
    }
    else if (slot == 2) {
        lib_info_ = true;
    }
    else if (slot == 3) {
        zt_library_refresh(library_);
    }
}

// LIBRARY's keys: UP/DOWN pick a pack, ENTER or 5 GET or REMOVE, 6 INFO,
// 7 RELOAD, ESC or 8 closes; a running job: ESC or 8 cancels it, any key
// closes its result.
bool TrackerScreen::library_input(uint32_t pressed, bool enter, bool escape, uint32_t soft, bool any) {
    if (lib_modal_) {
        uint32_t permille = 0;
        const uint8_t state = library_ ? zt_library_status(library_, &permille, nullptr, 0) : ZT_LIB_FAILED;
        const bool busy = state == ZT_LIB_DOWNLOADING || state == ZT_LIB_INSTALLING;
        if (busy && (escape || (soft & 8u))) zt_library_cancel(library_);
        else if (!busy && any) lib_modal_ = false;
        return true;
    }
    if (lib_info_) {
        if (any) lib_info_ = false;
        return true;
    }
    if (lib_confirm_remove_) {
        if (enter || (soft & 1u)) {
            lib_confirm_remove_ = false;
            ZtPackInfo info{};
            if (library_pack(lib_selected_, info)) {
                lib_job_ = info.name;
                zt_library_remove(library_, static_cast<uint32_t>(lib_selected_));
                lib_modal_ = true;
            }
        }
        else if (escape || (soft & 8u)) {
            lib_confirm_remove_ = false;
        }
        return true;
    }
    const int n = library_ ? static_cast<int>(zt_library_count(library_)) : 0;
    if (pressed & ZT_BTN_UP) lib_selected_ = std::max(0, lib_selected_ - 1);
    if (pressed & ZT_BTN_DOWN) lib_selected_ = std::min(std::max(0, n - 1), lib_selected_ + 1);
    if (enter) library_action(1);
    for (int i = 0; i < 3; ++i) {
        if (soft & (1u << i)) library_action(i + 1);
    }
    if (escape) {
        // ESC goes back to the first page, as on APP.
        close_library();
        page_ = 0;
        if (viewer_) zt_viewer_set_page(viewer_, 0);
    }
    return true;
}

// LIBRARY in the browser's look: an icon for what a pack holds, its name,
// its size (OK when installed); the header says the free space.
bool TrackerScreen::draw_library() {
    library_view_.set_visible(library_open_);
    if (!library_open_) return false;
    std::vector<BrowserView::Row> rows;
    const int n = library_ ? static_cast<int>(zt_library_count(library_)) : 0;
    for (int i = 0; i < n; ++i) {
        ZtPackInfo p{};
        if (!library_pack(i, p)) continue;
        const std::string kind = p.kind;
        using Kind = BrowserView::Kind;
        const Kind k = kind == "songs" ? Kind::Song : kind == "instruments" ? Kind::Instrument
                     : kind == "samples" ? Kind::Sample : Kind::Folder;
        // What the pack holds, e.g. "1 song · 12 instruments · 7 samples · CC0-1.0".
        std::string sub;
        auto part = [&](uint32_t count, const zt::i18n::Text& one, const zt::i18n::Text& many) {
            if (count == 0) return;
            if (!sub.empty()) sub += " · ";
            sub += std::to_string(count) + " " + tr(count == 1 ? one : many);
        };
        part(p.songs, {"song", "canzone", "morceau", "Song", "canción", "曲", "首歌曲"},
             {"songs", "canzoni", "morceaux", "Songs", "canciones", "曲", "首歌曲"});
        part(p.instruments, {"instrument", "strumento", "instrument", "Instrument", "instrumento", "音色", "个乐器"},
             {"instruments", "strumenti", "instruments", "Instrumente", "instrumentos", "音色", "个乐器"});
        part(p.samples, {"sample", "sample", "sample", "Sample", "sample", "サンプル", "个采样"},
             {"samples", "sample", "samples", "Samples", "samples", "サンプル", "个采样"});
        if (!sub.empty()) sub += " · ";
        sub += p.licence;
        rows.push_back({k, p.name,
                        p.installed ? std::string(tr({"INSTALLED", "INSTALLATO", "INSTALLÉ", "INSTALLIERT", "INSTALADO", "導入済み", "已安装"}))
                                    : pack_size(p.bytes),
                        true, p.installed ? 1 : 2, sub});
    }
    lib_selected_ = std::clamp(lib_selected_, 0, std::max(0, static_cast<int>(rows.size()) - 1));
    const int list_rows = library_view_.visible_rows();
    if (lib_selected_ < lib_top_) lib_top_ = lib_selected_;
    if (lib_selected_ >= lib_top_ + list_rows) lib_top_ = lib_selected_ - list_rows + 1;
    std::string message;
    uint32_t permille = 0;
    char msg[160] = {};
    const uint8_t state = library_ ? zt_library_status(library_, &permille, msg, sizeof msg) : ZT_LIB_FAILED;
    if (!library_) message = tr({"no library", "libreria assente", "pas de bibliothèque", "keine Bibliothek", "sin biblioteca", "ライブラリなし", "无资源库"});
    else if (state == ZT_LIB_FETCHING) message = tr({"loading...", "carico...", "chargement...", "lädt...", "cargando...", "読込中...", "加载中..."});
    else if (state == ZT_LIB_FAILED && !lib_modal_ && rows.empty()) message = tr({"offline", "offline", "hors ligne", "offline", "sin conexión", "オフライン", "离线"});
    const std::string free_space = library_ ? pack_size(zt_library_free_bytes(library_)) : "";
    library_view_.set("LIBRARY  " + free_space + " " + tr({"free", "liberi", "libres", "frei", "libres", "空き", "可用"}),
                      rows, lib_selected_, lib_top_, message);
    clear_grid();
    return true;
}


void TrackerScreen::copy_starter() {
    namespace fs = std::filesystem;
    std::error_code ec;
    const fs::path from = assets().resolve("content");
    const fs::path marker = fs::path(root_dir_) / ".packs" / "starter-0.1.0";
    if (from.empty() || !fs::is_directory(from, ec) || fs::exists(marker, ec)) return;
    int copied = 0;
    for (auto it = fs::recursive_directory_iterator(from, ec); !ec && it != fs::recursive_directory_iterator();
         it.increment(ec)) {
        if (!it->is_regular_file(ec)) continue;
        const fs::path to = fs::path(root_dir_) / fs::relative(it->path(), from, ec);
        if (ec || fs::exists(to, ec)) continue;
        fs::create_directories(to.parent_path(), ec);
        if (fs::copy_file(it->path(), to, fs::copy_options::skip_existing, ec)) ++copied;
    }
    fs::create_directories(marker.parent_path(), ec);
    std::ofstream(marker) << copied << " files\n";
}
} // namespace screen
