/*
 * Tracker screen: the home screen and the song browser (graphics), the song
 * pages of the navigator (ffi zt_viewer_*: TIMELINE, MIXER, TRACKS, SONG and
 * the M8 views) and the APP settings page. Key 4 switches the page; a bar at
 * the bottom names the pages.
 */
#pragma once

#include "bar_group.h"
#include "base_screen.h"
#include "browser_view.h"
#include "chain_phrase_view.h"
#include "instrument_view.h"
#include "eq_editor_view.h"
#include "sample_editor_view.h"
#include "home_view.h"
#include "spectrum.h"
#include "mixer_view.h"
#include "i18n/i18n.h"
#include "song_view.h"
#include "app_view.h"
#include "tracks_view.h"
#include "record_view.h"
#include "fx_picker_view.h"
#include "scope_view.h"
#include "timeline_view.h"
#include "zt_engine.h"
#include "zt_viewer.h"

#include <array>
#include <cstdint>
#include <memory>
#include <string>
#include <vector>

namespace screen {

// True while ESC has something to cancel: the app does not quit on ESC held.
bool exit_blocked();

class TrackerScreen : public BaseScreen {
public:
    TrackerScreen(viewmodel::BaseViewModel& view_model, app::AssetManager& assets);
    ~TrackerScreen() override;

protected:
    void build_content(lv_obj_t* content) override;
    bool full_screen() const override { return true; }

private:
    // The APP language, and a string in it (English when not translated).
    zt::i18n::Lang lang() const { return static_cast<zt::i18n::Lang>(settings_.language); }
    const char* tr(const zt::i18n::Text& text) const { return zt::i18n::pick(text, lang()); }
    void apply_language();
    struct Entry {
        std::string name;
        BrowserView::Kind kind;
        std::string detail;  // file size
        bool directory() const {
            return kind == BrowserView::Kind::Parent || kind == BrowserView::Kind::Folder;
        }
    };

    // Colours of a theme (APP page).
    // COLOR is the default; the others are monochrome: every colour of the
    // UI, meters and chain scale included, comes from the theme's hue.
    struct Theme {
        const char* name;
        uint32_t bg, fg, cursor, selection, bar_fg, meter, accent, playing, muted, solo, dim;
        // hi: bright labels (song name, bottom bar); badge and on_badge: track
        // badges and their numbers; alert and on_alert: playing chain, selected
        // strip and mute fills and the text on them; panel: alternate lanes;
        // unlit: meter segments off; on_solo: text on the solo fill.
        uint32_t hi, badge, on_badge, alert, on_alert, panel, unlit, on_solo;
        std::array<uint32_t, 5> chain;    // chain scale, 00 to 8F
        std::array<uint32_t, 4> battery;  // from 60%, 30%, 15%, below
        bool colorful;
    };

    // App settings, stored in ~/.config/zerotracker/settings.ini.
    struct Settings {
        int font_size{14};
        int theme{0};
        bool spectrum{true};
        int language{0};  // zt::i18n::Lang: EN IT FR DE ES JA ZH (from the locale until set)
        std::string last_song;  // RESUME on the home screen
        std::string library_url;  // LIBRARY's catalog, when not the default
    };
    // On the first start, the starter content the package carries is copied
    // into the songs folder (files already there stay).
    void copy_starter();

    static void tick_cb(lv_timer_t* timer);
    void tick();
    void handle_browser(uint32_t pressed);
    void list_directory();
    void open_entry();
    void redraw();
    void draw_lines(const std::vector<std::string>& lines, int cursor_line, int cursor_col,
                    int cursor_width);

    // (Re)creates every widget of the screen with the current font and theme.
    void build_grid();
    const Theme& theme() const;
    void load_settings();
    void save_settings() const;

    // Pages: 0-3 are the navigator's (ZT_PAGE_*), 4 is APP.
    static constexpr int kPageApp = 4;
    // OSCILLOSCOPES, opened with OSCIL from the MIXER (not on the key-4 cycle).
    static constexpr int kPageOscil = 5;
    // LIBRARY, after APP on the key-4 cycle.
    static constexpr int kPageLibrary = 6;
    void show_page_library();
    // MANUAL (APP, key 8): a QR code to the getting-started guide and its address.
    lv_obj_t* manual_panel_{nullptr};
    bool manual_open_{false};
    int page_{0};
    void next_page();
    void previous_page();
    int app_field_{0};
    void handle_app(uint32_t held, uint32_t pressed);
    // How far APP is scrolled, in px (past LANGUAGE: the instructions).
    int app_scroll_{0};
    void update_bottom_bar();
    void draw_highlights(const std::vector<std::string>& lines, int cursor_line, int cursor_col,
                         int cursor_width);
    void update_mixer_bars();

    lv_obj_t* content_{nullptr};
    Settings settings_;

    // Grid size from the font: cols_ x rows_count_ cells of cell_w_ x cell_h_ px.
    const lv_font_t* font_{nullptr};
    const lv_font_t* bold_font_{nullptr};
    int cell_w_{8};
    int cell_h_{8};
    int cols_{40};
    int rows_count_{21};
    int top_px_{0};
    int letter_space_{0};
    std::vector<lv_obj_t*> rows_;

    // Top bar (spectrum band): song name and tempo left, battery right.
    lv_obj_t* bar_left_{nullptr};
    lv_obj_t* bar_right_{nullptr};
    // Tempo, right-aligned just left of the track meters.
    lv_obj_t* bar_bpm_{nullptr};
    std::string shown_bpm_;
    uint32_t battery_checked_at_{0};
    void update_bar();

    // Bottom bar, as ZeroRadio's NavBar: five equal slots. Slot 0 (key 4)
    // shows the page number and name, slots 1-4 the page's buttons for keys
    // 5-8 (icons in Phosphor Fill).
    struct NavSlot {
        const char* text{""};
        bool icon{true};
        bool enabled{true};
        const char* key{""};  // the English name, for the SYM tooltip
    };
    // SYM+4..8: a small box saying what that key does here.
    lv_obj_t* hint_box_{nullptr};
    uint32_t hint_shown_at_{0};
    void show_key_hint(int key);
    void close_key_hint();
    static constexpr int kNavHeight = 26;
    // Bold font of the bottom-bar labels, larger than the grid's.
    const lv_font_t* nav_font_{nullptr};
    std::array<NavSlot, 5> nav_slots() const;
    void soft_key(int slot);
    lv_obj_t* nav_bg_{nullptr};
    std::array<lv_obj_t*, 5> nav_labels_{};
    std::array<std::string, 5> nav_shown_{};
    const lv_font_t* icon_font_{nullptr};
    uint32_t last_soft_{0};

    // Highlight overlays (bold playing chains, coloured labels), reused.
    struct Overlay {
        lv_obj_t* label{nullptr};
        std::string text;
        int x{-1}, y{-1}, style{0};
        uint32_t bg{0};
        bool visible{false};
    };
    std::vector<Overlay> overlays_;

    // TIMELINE drawn as graphics (lanes, badges, coloured chains).
    TimelineView timeline_;
    bool draw_timeline();
    void clear_grid();
    // CHAIN | PHRASE side by side, drawn as graphics.
    ChainPhraseView chain_phrase_;
    bool draw_chain_phrase();
    // Sample editor, from INSTRUMENT (ENTER on EDIT by the sample).
    SampleEditorView sample_view_;
    ZtSampleEdit* sample_edit_{nullptr};
    std::string sample_path_;
    int se_mode_{0};    // 0 selection, 1 loop, 2 slices
    int se_active_{1};  // which end of the selection or loop moves: 0 start, 1 end
    uint32_t se_from_{0}, se_to_{0};  // the frames on screen
    bool se_menu_{false};
    int se_menu_index_{0};
    int se_slice_parts_{16};
    bool se_naming_{false};
    std::string se_name_;
    bool se_confirm_exit_{false};
    bool se_playing_{false};
    uint32_t se_play_from_{0}, se_play_to_{0}, se_play_tick_{0};
    std::string se_message_;
    uint32_t se_message_at_{0};
    void open_sample_editor(const std::string& path);
    void close_sample_editor();
    bool draw_sample_editor();
    // True when something changed (the screen redraws only then).
    bool sample_editor_input(uint32_t pressed, uint32_t held, bool enter, bool escape, bool del,
                             uint32_t soft, const std::string& typed, bool ctrl);
    // LEFT/RIGHT held: repeats after 300 ms, faster the longer it is held.
    uint32_t se_hold_since_{0}, se_hold_last_{0};
    // EQ editor, from INSTRUMENT (ENTER on EQ): the response curve, a handle
    // per band.
    EqEditorView eq_view_;
    bool eq_open_{false};
    int eq_band_{0};
    bool eq_changed_{true};     // a key since the last EQ frame
    uint32_t eq_drawn_at_{0};
    uint32_t eq_hold_since_{0}, eq_hold_last_{0};
    void open_eq_editor();
    void close_eq_editor();
    bool draw_eq_editor();
    bool eq_editor_input(uint32_t pressed, uint32_t held, bool enter, bool escape, bool del,
                         uint32_t soft, const std::string& typed, bool ctrl);
    // SLICE mode in two steps: choose a slice with
    // LEFT/RIGHT, ENTER to edit it: the arrows move its START or END (ENTER
    // switches), ESC goes back to choosing.
    int se_slice_{0};
    bool se_slice_edit_{false};
    int se_slice_edge_{0};
    std::pair<uint32_t, uint32_t> se_slice_bounds() const;
    uint32_t se_marker() const;
    void se_move_marker(int64_t frames);
    void se_play();
    void se_save();
    void se_say(const std::string& message);
    // INSTRUMENT with bold labels and value bars.
    InstrumentView instrument_;
    bool draw_instrument();
    // MIXER and OSCILLOSCOPES drawn as graphics.
    MixerView mixer_view_;
    bool draw_mixer();
    // SONG as graphics.
    SongView song_view_;
    bool draw_song();
    // APP as graphics.
    AppView app_view_;
    void draw_app();
    // TRK as graphics.
    TracksView tracks_view_;
    bool draw_tracks();
    // The APP font size, and a size designed at 14 px moved by as many
    // pixels as the APP size is from 14.
    int ui_font_size_{14};
    int ui_px(int base) const { return std::max(8, base + ui_font_size_ - 14); }
    // SONG: RENDER to WAV, on its own thread (render.rs).
    ZtRender* render_{nullptr};
    std::string render_text_;
    bool render_failed_{false};
    bool render_modal_{false};  // the progress modal, until a key closes the result
    int render_choice_{-1};     // the modal's question: 0 MIX, 1 STEMS, 2 both; -1 none
    void start_render(int choice);
    bool render_modal_key(uint32_t pressed, bool enter, bool escape, uint32_t soft, bool any);
    void update_render();
    // The FX command picker as a grid.
    FxPickerView fx_picker_view_;
    bool draw_fx_picker();
    // REC. (record a sample for the SAMPLER).
    RecordView rec_view_;
    bool rec_open_{false};
    std::string rec_error_;
    void toggle_recording();
    void close_recorder(bool keep);
    bool draw_recorder();
    ScopeView scopes_;
    std::vector<float> track_tap_;
    bool draw_scopes();

    // Spectrum of the master output behind the bottom bar (engine output tap).
    std::unique_ptr<Spectrum> spectrum_;
    std::vector<float> tap_;
    BarGroup spectrum_bars_;
    void update_spectrum();
    // A finer one for the EQ editor's graph (2048 frames, one column per pixel).
    std::unique_ptr<Spectrum> eq_spectrum_;
    std::vector<float> eq_tap_;

    // MIXER page: one bar per strip (8 tracks and master).
    BarGroup mixer_bars_;
    std::array<int32_t, 4> mixer_area_{};

    lv_obj_t* cursor_{nullptr};
    lv_obj_t* selection_{nullptr};
    lv_timer_t* timer_{nullptr};
    uint32_t last_buttons_{0};
    bool last_escape_{false};
    bool last_undo_{false};
    bool last_redo_{false};
    bool last_mute_{false};
    bool last_vol_down_{false};
    bool last_vol_up_{false};
    bool last_page_{false};
    // Questions in a small modal at the centre: SAVE
    // changes before closing, overwrite an instrument file.
    lv_obj_t* modal_{nullptr};
    lv_obj_t* modal_title_{nullptr};
    lv_obj_t* modal_body_{nullptr};
    lv_obj_t* modal_keys_{nullptr};
    lv_obj_t* modal_bar_{nullptr};  // RENDER's progress
    int modal_base_h_{86};          // height with a one-line body
    const lv_font_t* modal_keys_font_{nullptr};
    std::string modal_shown_;
    void update_modal();
    // Key 5 (EDIT) held with an arrow: no edit-mode toggle on its release.
    bool soft_edit_used_{false};
    // Key 4: a tap goes to the next page on release, holding it goes back.
    uint32_t page_down_at_{0};
    bool page_hold_done_{false};
    // Volume shown in the top bar for a moment after a volume key.
    std::string volume_text_;
    uint32_t volume_shown_at_{0};
    // ESC with unsaved edits asks first: EDIT saves, OPTION discards, ESC cancels.
    bool confirm_close_{false};
    // Text entry for a NAME field: EDIT on it opens, ENTER writes, ESC
    // cancels, DEL deletes; the text shows in the top bar.
    bool text_mode_{false};
    std::string text_;
    int text_max_{0};
    std::string last_chars_;
    bool last_enter_{false};
    bool open_text_entry();
    // Text entry happens in the field itself: the field
    // under the cursor shows the text being typed and a '_' caret, as wide
    // as the longest text it takes.
    void show_text_entry(std::string& line, int col, int& width) const;
    // File to open once the current song is closed (NEW on the SONG page).
    std::string open_after_close_;
    void close_song();
    void after_input();

    // Without a song, page 0 is HOME, or LOAD (the browser) after LOAD.
    HomeView home_;
    BrowserView browser_;
    bool browsing_{false};
    BrowserView::Kind browse_for_{BrowserView::Kind::Song};
    bool openable(const Entry& e) const;
    // PLAY (key 6) in LOAD: the selected file of the kind LOAD opens.
    ZtPreview* preview_{nullptr};
    bool preview_shown_playing_{false};
    bool previewable(const Entry& e) const;
    // LOAD from an open song (INSTRUMENT: LOAD or the sample): the song
    // stops, and is published to the engine again once LOAD closes if a
    // preview took its place there.
    void start_browse(BrowserView::Kind kind);
    void end_browse();
    bool preview_took_engine_{false};
    // EDIT+PLAY on INSTRUMENT, PLAY on a POOL row: the instrument's arpeggio
    // plays in place of the song until any key.
    bool instrument_preview_{false};
    void end_instrument_preview();
    void toggle_preview();
    void stop_preview();
    std::string home_message_;  // e.g. NEW FAILED, in place of NEW's detail
    std::string browser_message_;
    uint32_t browser_message_at_{0};
    uint32_t last_animation_{0};
    std::array<HomeView::Item, 4> home_items() const;
    void handle_home(uint32_t pressed);
    void home_action(int item);
    bool draw_home();
    bool draw_browser();

    // LIBRARY: packs of songs, instruments and samples to download (library.rs),
    // opened from HOME and APP. The list reuses the browser's look.
    BrowserView library_view_;
    ZtLibrary* library_{nullptr};
    bool library_open_{false};
    int lib_selected_{0}, lib_top_{0};
    uint8_t lib_shown_state_{0};
    bool lib_info_{false};           // the selected pack's details in the modal
    bool lib_confirm_remove_{false};  // REMOVE asks first
    bool lib_modal_{false};          // a download, install or removal, until a key closes it
    std::string lib_job_;            // the pack it is about
    void open_library();
    void close_library();
    bool library_pack(int index, ZtPackInfo& out) const;
    void library_action(int slot);
    bool library_input(uint32_t pressed, bool enter, bool escape, uint32_t soft, bool any);
    bool draw_library();

    std::string root_dir_;
    std::string dir_;
    std::vector<Entry> entries_;
    int selected_{0};
    int top_{0};
    std::string error_;
    ZtViewer* viewer_{nullptr};

    // Audio: one engine for the screen's life, rendering through the backend.
    ZtEngine* engine_{nullptr};
    BarGroup meters_;
    uint32_t last_redraw_{0};

    // Last text/size set on each widget, to skip unchanged updates.
    std::vector<std::string> shown_;
    std::string shown_bar_;

    void open_song(const std::string& path);
    void update_meters();

    // Development aids: ZT_OPEN=<file> opens a song at start, ZT_KEYS plays
    // a key script (see run_script_step).
    std::vector<std::string> script_;
    size_t script_pos_{0};
    uint32_t script_wait_until_{0};
    void run_script_step();
};

} // namespace screen
