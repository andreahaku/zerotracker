/* C API of the read-only song viewer (ffi/src/viewer.rs). */
#ifndef ZT_VIEWER_H
#define ZT_VIEWER_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* M8 buttons, as bits of the held/pressed masks. */
enum {
    ZT_BTN_UP = 1u << 0,
    ZT_BTN_DOWN = 1u << 1,
    ZT_BTN_LEFT = 1u << 2,
    ZT_BTN_RIGHT = 1u << 3,
    ZT_BTN_SHIFT = 1u << 4,
    ZT_BTN_PLAY = 1u << 5,
    ZT_BTN_OPTION = 1u << 6,
    ZT_BTN_EDIT = 1u << 7,
};

typedef struct Viewer ZtViewer;

/* Opens a .m8s file. NULL on error, with a message in err (err_len bytes). */
ZtViewer *zt_viewer_open(const char *path, char *err, size_t err_len);
void zt_viewer_close(ZtViewer *viewer);

/* One input poll: held = buttons down now, pressed = buttons that went down. */
void zt_viewer_input(ZtViewer *viewer, uint32_t held, uint32_t pressed);

/* Fills rows lines of cols chars, each NUL-terminated (rows * (cols + 1)
 * bytes). Returns the lines written. Cursor line/column/width are -1 when
 * the view has no cursor. */
size_t zt_viewer_frame(const ZtViewer *viewer, char *out, size_t rows, size_t cols,
                       int32_t *cursor_line, int32_t *cursor_col, int32_t *cursor_width);

/* Saves in place (backup + atomic rename). 1 on success. */
int32_t zt_viewer_save(ZtViewer *viewer);

/* Undo (redo = 0) or redo (redo = 1). 1 when something changed. */
int32_t zt_viewer_undo(ZtViewer *viewer, int32_t redo);

/* Replaces the instrument under the cursor with a .m8i file, or points the
 * SAMPLER under the cursor at a .wav under the songs root. Undoable. 1 on
 * success; else the viewer's message says why. */
int32_t zt_viewer_load_instrument(ZtViewer *viewer, const char *path);
int32_t zt_viewer_set_sample(ZtViewer *viewer, const char *path);

void zt_viewer_reload_samples(ZtViewer *viewer);

/* Sample editor: one .wav in memory with a selection, a loop region (WAV
 * smpl chunk) and up to 128 slice markers (cue chunk). Processes work on
 * the selection (or the whole sample when it is empty). */
typedef struct ZtSampleEdit ZtSampleEdit;
typedef struct ZtEngine ZtEngine;
typedef struct {
    uint32_t frames;
    uint32_t rate;
    uint16_t channels;
    uint16_t bits;
    uint32_t sel_start;
    uint32_t sel_end;
    uint32_t loop_start; /* both 0xFFFFFFFF: no loop */
    uint32_t loop_end;
    uint32_t slices;
    uint8_t dirty;
    uint8_t can_undo;
    uint8_t can_redo;
} ZtSampleInfo;
enum {
    ZT_SAMPLE_CROP = 0, ZT_SAMPLE_DELETE, ZT_SAMPLE_DUPLICATE, ZT_SAMPLE_NORMALIZE,
    ZT_SAMPLE_SILENCE, ZT_SAMPLE_REVERSE, ZT_SAMPLE_INVERT, ZT_SAMPLE_FADE_IN, ZT_SAMPLE_FADE_OUT,
    ZT_SAMPLE_XFADE_LOOP, ZT_SAMPLE_MONO_MIX, ZT_SAMPLE_MONO_LEFT, ZT_SAMPLE_MONO_RIGHT,
    ZT_SAMPLE_DOWNSAMPLE, ZT_SAMPLE_BITS_16, ZT_SAMPLE_BITS_8, ZT_SAMPLE_SLICE_EVEN,
    ZT_SAMPLE_SLICE_AUTO, ZT_SAMPLE_SLICE_SILENCE, ZT_SAMPLE_CLEAR_SLICES,
};
ZtSampleEdit *zt_sample_open(const char *path, char *err, size_t err_len);
void zt_sample_close(ZtSampleEdit *edit);
void zt_sample_info(ZtSampleEdit *edit, ZtSampleInfo *out);
size_t zt_sample_slices(ZtSampleEdit *edit, uint32_t *out, size_t max);
void zt_sample_peaks(ZtSampleEdit *edit, uint32_t from, uint32_t to, size_t columns, float *lo,
                     float *hi);
void zt_sample_set_selection(ZtSampleEdit *edit, uint32_t a, uint32_t b);
void zt_sample_set_loop(ZtSampleEdit *edit, uint32_t a, uint32_t b);
void zt_sample_add_slice(ZtSampleEdit *edit, uint32_t at);
void zt_sample_remove_slice(ZtSampleEdit *edit, size_t index);
/* Moves slice `index` (kept between its neighbours); begin_edit makes one
 * undo step for a run of moves. */
void zt_sample_move_slice(ZtSampleEdit *edit, size_t index, uint32_t at);
void zt_sample_begin_edit(ZtSampleEdit *edit);
int32_t zt_sample_process(ZtSampleEdit *edit, int32_t op, int32_t param, char *err, size_t err_len);
int32_t zt_sample_undo(ZtSampleEdit *edit, int32_t redo);
int32_t zt_sample_preview(ZtSampleEdit *edit, ZtEngine *engine, uint32_t from, uint32_t to);
void zt_sample_preview_stop(ZtEngine *engine);
/* Over the file (path NULL) or to path; the saved path (or the error) in out. */
int32_t zt_sample_save(ZtSampleEdit *edit, const char *path, char *out, size_t len);

/* NEW without an open song: NEW00.m8s (or the first free NEWnn) in `dir`, from
   TEMPLATE.m8s in `root` if there is one, else generated from the format model. Writes the path, or an error, into
   `out`. 1 on success. */
int32_t zt_new_song(const char *dir, const char *root, char *out, size_t out_len);

/* Writes unsaved edits to NAME.autosave, original untouched. 1 on success. */
int32_t zt_viewer_autosave(const ZtViewer *viewer);

typedef struct ZtEngine ZtEngine;

/* Attaches the app's engine (must outlive the viewer) and the songs root;
 * loads the song's samples and publishes it for playback. PLAY then starts
 * or stops playback from the cursor. */
void zt_viewer_attach_engine(ZtViewer *viewer, ZtEngine *engine, const char *root);

/* CHAIN | PHRASE side by side: the chain on the left, the phrase of its
 * selected row (or the phrase being edited) on the right. Cell texts as on
 * the M8; roles pick the colours. */
enum {
    ZT_CELL_EMPTY = 0,
    ZT_CELL_PHRASE = 1,
    ZT_CELL_TRANSPOSE = 2,
    ZT_CELL_NOTE = 3,
    ZT_CELL_VELOCITY = 4,
    ZT_CELL_INSTRUMENT = 5,
    ZT_CELL_FX_COMMAND = 6,
    ZT_CELL_FX_VALUE = 7,
};
typedef struct {
    char text[8];
    uint8_t role;
} ZtSplitCell;
typedef struct {
    uint8_t chain;
    uint8_t phrase;
    uint8_t phrase_valid; /* 0: the selected chain row has no phrase */
    uint8_t focus;        /* 0: cursor in the chain, 1: in the phrase */
    uint8_t chain_row;
    uint8_t chain_field;
    uint8_t phrase_row;
    uint8_t phrase_field;
    int8_t chain_play; /* row playing on the cursor's track, or -1 */
    int8_t phrase_play;
    ZtSplitCell chain_cells[16][2];  /* phrase, transpose */
    ZtSplitCell phrase_cells[16][9]; /* note, vel, instr, 3 x (fx, value);
                                        TABLE: tsp, vol, 3 x (fx, value) */
    uint8_t table; /* 1: a TABLE (number in `phrase`), shown alone */
} ZtChainPhrase;
/* 1 and `out` filled when the cursor is in a chain, a phrase or a table (no
 * picker). */
int32_t zt_viewer_chain_phrase(const ZtViewer *viewer, ZtChainPhrase *out);

/* The FX command picker for the graphic view: the commands as a grid of
 * `columns` (the arrows move on it), three-letter names. */
typedef struct {
    uint8_t count;
    uint8_t selected;
    uint8_t columns;
    uint8_t ids[96];
    char names[96][4];
} ZtFxPicker;
/* 1 and `out` filled when the FX command picker is open. */
int32_t zt_viewer_fx_picker(const ZtViewer *viewer, ZtFxPicker *out);

/* INSTRUMENT for the graphic view: the visible text lines (as the text view
 * scrolls them) and each value's place; kind 0 text, 1 number 00-FF (bar
 * from the left), 2 number centred on 80 (bar from the middle). */
typedef struct {
    uint8_t line;
    uint8_t column;
    uint8_t width;
    uint8_t kind;
} ZtPageSpan;
typedef struct {
    uint8_t line_count;
    uint8_t span_count;
    int8_t cursor_line; /* -1: no cursor */
    uint8_t cursor_column;
    uint8_t cursor_width;
    char lines[24][48];
    ZtPageSpan spans[64];
} ZtInstrumentPage;
/* 1 and `out` filled when the cursor is on an instrument (no picker). */
int32_t zt_viewer_instrument_page(const ZtViewer *viewer, size_t rows, ZtInstrumentPage *out);

/* ZeroTracker's keys (in place of the M8's OPTION / SHIFT combinations).
 * ENTER: 0 nothing, 1 done, 2 open text entry for the field. ESC: 1 when
 * handled (an edit undone, a selection or the FX list closed, a level up),
 * 0 on the song level. State bits for the bottom bar. */
enum {
    ZT_UX_EDITING = 1,
    ZT_UX_SELECTING = 2,
    ZT_UX_CAN_OPEN = 4,
    ZT_UX_INSTRUMENT = 8,
    ZT_UX_PICKER = 16,
    ZT_UX_CONFIRM_OVERWRITE = 32, /* SAVE again overwrites, ESC keeps the file */
};
void zt_viewer_overwrite_name(const ZtViewer *viewer, char *out, size_t len);
int32_t zt_viewer_ux_enter(ZtViewer *viewer);
int32_t zt_viewer_ux_back(ZtViewer *viewer);
uint32_t zt_viewer_ux_state(const ZtViewer *viewer);
void zt_viewer_ux_edit(ZtViewer *viewer);
void zt_viewer_ux_open(ZtViewer *viewer);
void zt_viewer_ux_select(ZtViewer *viewer);
void zt_viewer_ux_copy(ZtViewer *viewer);
void zt_viewer_ux_paste(ZtViewer *viewer);
void zt_viewer_ux_clear(ZtViewer *viewer);
void zt_viewer_ux_load(ZtViewer *viewer);
void zt_viewer_ux_save(ZtViewer *viewer);

/* PLAY in the LOAD browser: hear a file before loading it, one at a time.
 * Kinds: 0 song (from its first row), 1 instrument (.m8i, a C major
 * arpeggio in an empty song), 2 sample (.wav, once). The engine must outlive
 * the preview; call zt_preview_tick on every UI tick. */
typedef struct ZtPreview ZtPreview;
enum { ZT_PREVIEW_SONG = 0, ZT_PREVIEW_INSTRUMENT = 1, ZT_PREVIEW_SAMPLE = 2 };
ZtPreview *zt_preview_create(ZtEngine *engine, const char *root);
void zt_preview_destroy(ZtPreview *preview);
/* 1 on success, else a short error in `err` (when not NULL). */
int32_t zt_preview_play(ZtPreview *preview, const char *path, int32_t kind, char *err,
                        size_t err_len);
void zt_preview_stop(ZtPreview *preview);
void zt_preview_tick(ZtPreview *preview);
int32_t zt_preview_playing(const ZtPreview *preview);
/* The song preview of `path`, stopped, as a viewer to open without loading
 * it again; NULL when the preview plays something else. */
ZtViewer *zt_preview_take_song(ZtPreview *preview, const char *path);
/* Instrument `slot` of the viewer's song, with its table and EQ, as the
 * arpeggio preview (looping until zt_preview_stop). The song is replaced in
 * the engine: attach the viewer again afterwards. 1 on success, else a
 * short error in `err` (when not NULL). */
int32_t zt_preview_play_instrument(ZtPreview *preview, const ZtViewer *viewer, uint8_t slot,
                                   char *err, size_t err_len);

/* Call every UI frame while a song is open (snapshot hand-off). */
void zt_viewer_tick(ZtViewer *viewer);

/* Stops playback. */
void zt_viewer_stop(ZtViewer *viewer);

/* Online content library. Fixed-size strings are always NUL-terminated. */
typedef struct Library ZtLibrary;
typedef struct {
    char id[64];
    char name[96];
    char kind[16];
    char authors[128];
    char licence[32];
    char licence_url[256];
    char description[256];
    uint64_t bytes;
    uint32_t songs;
    uint32_t instruments;
    uint32_t samples;
    uint8_t installed;
} ZtPackInfo;
enum {
    ZT_LIB_IDLE = 0,
    ZT_LIB_FETCHING = 1,
    ZT_LIB_DOWNLOADING = 2,
    ZT_LIB_INSTALLING = 3,
    ZT_LIB_DONE = 4,
    ZT_LIB_FAILED = 5,
    ZT_LIB_CANCELLED = 6,
};
ZtLibrary *zt_library_open(const char *root, const char *catalog_url);
void zt_library_refresh(ZtLibrary *lib);
uint32_t zt_library_count(const ZtLibrary *lib);
int32_t zt_library_pack(const ZtLibrary *lib, uint32_t index, ZtPackInfo *out);
int32_t zt_library_install(ZtLibrary *lib, uint32_t index);
int32_t zt_library_remove(ZtLibrary *lib, uint32_t index);
void zt_library_cancel(ZtLibrary *lib);
uint8_t zt_library_status(const ZtLibrary *lib, uint32_t *permille, char *msg, size_t len);
uint64_t zt_library_free_bytes(const ZtLibrary *lib);
void zt_library_free(ZtLibrary *lib);

/* The 8 track peak levels, 0..65535. */
void zt_viewer_levels(const ZtViewer *viewer, uint16_t *out);

/* While selecting: out = first line, last line, first column, end column,
 * in screen lines of a rows x cols grid (as the cursor). Returns 1, else 0. */
int32_t zt_viewer_selection(const ZtViewer *viewer, size_t rows, size_t cols, int32_t *out);

/* TIMELINE for the app's graphic view. Rows are song rows, -1 means none. */
#define ZT_TIMELINE_MAX 64
typedef struct {
    int32_t first_row;      /* first visible song row */
    int32_t count;          /* visible song rows filled in cells */
    int32_t cursor_row;
    int32_t cursor_track;
    int32_t sel[4];         /* selection: first row, last row, first track, last track */
    int32_t playing[8];     /* song row each track plays */
    uint8_t mute, solo;     /* track bit masks */
    uint8_t cells[8 * ZT_TIMELINE_MAX]; /* chain at [track * ZT_TIMELINE_MAX + i], 0xFF empty */
} ZtTimeline;

/* Fills `out` with `visible` song rows (at most ZT_TIMELINE_MAX) from the
 * scroll position that keeps the cursor on screen. Returns 1 on the TIMELINE
 * page, else 0. */
int32_t zt_viewer_timeline(const ZtViewer *viewer, size_t visible, ZtTimeline *out);

/* MIXER for the app's graphic view. Strip 8 is the master. */
typedef struct {
    int32_t selected;
    int32_t limit_mode;     /* 1 while LIMIT is on: the arrows move the limiter */
    uint8_t volumes[9];     /* track volumes 0-7, master 8 */
    uint8_t mute, solo;     /* track bit masks */
    uint8_t limiter;        /* mixer limiter level */
} ZtMixer;

/* Fills `out` on the MIXER page and returns 1, else 0. */
int32_t zt_viewer_mixer(const ZtViewer *viewer, ZtMixer *out);

/* SONG for the graphic view (graphic_pages.rs). field: 0 NAME, 1 TEMPO,
 * 2 TRANSPOSE, 3 SAVE, 4 NEW, 5 LOAD, 6 RENDER. rows/chains/phrases/instruments: what
 * the song uses. 1 when SONG is on screen. */
typedef struct {
    char name[16];
    char file[48];
    float tempo;
    uint8_t transpose;
    uint8_t field;
    uint8_t editing;
    uint8_t dirty;
    uint16_t rows;
    uint16_t chains;
    uint16_t phrases;
    uint16_t instruments;
} ZtSongPage;
int32_t zt_viewer_song_page(const ZtViewer *viewer, ZtSongPage *out);

/* TRK for the graphic view: the eight tracks, then a window of the POOL.
 * kind: 0 none, 1 WAVSYNTH, 2 MACROSYNTH, 3 SAMPLER, 4 FMSYNTH, 5 HYPERSYNTH,
 * 6 MIDI OUT, 7 EXTERNAL. flags: bit 0 muted, bit 1 soloed, bit 2 playing.
 * cursor: 0-7 a track, 8+ pool slot cursor - 8; column: 0 NAME, 1 DRY, 2 MFX,
 * 3 DEL, 4 REV, 5 EQ. 1 when TRK is on screen. */
typedef struct {
    int16_t instrument;
    uint8_t kind;
    uint8_t flags;
    char name[16];
    char detail[16];
    uint8_t sends[4];
    uint8_t eq;
    char note[4]; /* the note the track plays now (C-4, OFF), empty when stopped */
} ZtTrackRow;
enum { ZT_POOL_ROWS = 6 };
typedef struct {
    ZtTrackRow tracks[8];
    ZtTrackRow pool[ZT_POOL_ROWS];
    uint8_t pool_first;
    uint16_t cursor;
    uint8_t column;
    uint8_t editing;
} ZtTracksPage;
int32_t zt_viewer_tracks_page(const ZtViewer *viewer, ZtTracksPage *out);

/* Pages (key 4): TIMELINE is the SONG view turned 90 degrees; CHAIN, PHRASE,
 * INSTRUMENT and TABLE open from it. APP is drawn by the app itself. */
enum {
    ZT_PAGE_TIMELINE = 0,
    ZT_PAGE_MIXER = 1,
    ZT_PAGE_TRACKS = 2,
    ZT_PAGE_SONG = 3,
};
int32_t zt_viewer_page(const ZtViewer *viewer);
void zt_viewer_set_page(ZtViewer *viewer, int32_t page);

/* Highlight styles drawn over the text grid. */
enum {
    ZT_STYLE_PLAYING = 1, /* bold, bright: the chain a track is playing */
    ZT_STYLE_ACCENT = 2,  /* track labels */
    ZT_STYLE_MUTED = 3,
    ZT_STYLE_SOLO = 4,
    ZT_STYLE_DIM = 5,
};

/* Highlights of a rows x cols grid: up to max spans of 4 values (line,
 * column, width, style) into out. Returns the number of spans. */
size_t zt_viewer_highlights(const ZtViewer *viewer, size_t rows, size_t cols, int32_t *out,
                            size_t max);

/* MIXER page: meter area as first line, last line, first column, column step
 * of the 9 strips (tracks 1-8, master). Returns 1 on the MIXER page, else 0. */
int32_t zt_viewer_meter_area(const ZtViewer *viewer, int32_t *out);

/* Keys 5-8 as slots 1-4 of the bottom bar on MIXER, TRACKS and SONG.
 * Returns 1 when the page did something with it. */
int32_t zt_viewer_soft_key(ZtViewer *viewer, int32_t slot);

/* ENTER on the TRK page: opens the row's instrument (a track's, or a pool
 * row's NAME), or toggles edit mode on a pool value (DRY MFX DEL REV EQ). */
void zt_viewer_tracks_enter(ZtViewer *viewer);

/* Text field under the cursor (song or instrument NAME): its value goes to out,
 * the return value is its maximum length, 0 when there is none. */
int32_t zt_viewer_text_target(const ZtViewer *viewer, char *out, size_t len);

/* Sets the text field under the cursor. Returns 1 when the song changed. */
int32_t zt_viewer_set_text(ZtViewer *viewer, const char *text);

/* 1 while the song plays. */
int32_t zt_viewer_playing(const ZtViewer *viewer);

/* Request: 0 none, 1 close the song (SONG page LOAD), 2 open the file whose
 * path is written into out (NEW), 3 browse for an instrument (INSTRUMENT
 * LOAD), 4 browse for a sample (ENTER on a SAMPLER's sample), 5 open the
 * sample editor on the file written into out (ENTER on EDIT by the sample),
 * 6 open the EQ editor (ENTER on an instrument's EQ), 7 record a sample for
 * the SAMPLER on screen (REC. on an empty SAMPLE), 8 render the song to
 * WAV (SONG: RENDER, zt_render_start), 0x100 | slot preview
 * that instrument (INSTRUMENT: EDIT+PLAY; POOL: PLAY while stopped). */
int32_t zt_viewer_take_request(ZtViewer *viewer, char *out, size_t len);

/* RENDER (SONG page): the song to WAV files under <root>/Renders/<SONG>/,
 * the mix and STEMS/<SONG>_<n>.wav per track in use, 44.1 kHz 16-bit
 * stereo, on a thread and an engine of its own. */
typedef struct Render ZtRender;
enum { ZT_RENDER_RUNNING = 1, ZT_RENDER_DONE = 2, ZT_RENDER_FAILED = 3, ZT_RENDER_CANCELLED = 4 };
enum { ZT_RENDER_MIX = 1, ZT_RENDER_STEMS = 2 };
/* write: ZT_RENDER_MIX, ZT_RENDER_STEMS or both. NULL when there is nothing
 * to render. */
ZtRender *zt_render_start(const ZtViewer *viewer, const char *root, uint8_t write);
/* State (ZT_RENDER_*); passes finished (0 while the mix renders) of
 * passes; progress over all of them in thousandths. Out pointers may be
 * NULL. */
uint8_t zt_render_status(const ZtRender *render, uint8_t *pass, uint8_t *passes, uint32_t *permille);
/* Cancels a running render, waits for it and frees it. */
void zt_render_free(ZtRender *render);

/* EQ editor: the current instrument's three-band EQ. */
typedef struct ZtEqBand {
    uint8_t kind;   /* LOWCUT, LOWSHELF, BELL, BANDPASS, HI.SHELF, HI.CUT, ALLPASS */
    uint8_t mode;   /* STEREO, MID, SIDE, LEFT, RIGHT */
    uint8_t q;      /* 1-99, 50 = 0.707 */
    uint16_t freq;  /* Hz */
    int16_t gain;   /* hundredths of a dB */
} ZtEqBand;

typedef struct ZtEqState {
    int32_t index;  /* EQ number 00-7F, or -1 */
    ZtEqBand bands[3];
} ZtEqState;

enum { ZT_EQ_GAIN = 0, ZT_EQ_FREQ, ZT_EQ_Q, ZT_EQ_TYPE, ZT_EQ_MODE };

/* Opens the instrument's EQ, giving it its own number when it has none.
 * Returns the EQ number, or -1. */
int32_t zt_viewer_eq_open(ZtViewer *viewer);
void zt_viewer_eq_state(const ZtViewer *viewer, ZtEqState *out);
/* Tracks playing the instrument whose EQ is open (bit 0 = track 1). */
uint8_t zt_viewer_eq_tracks(const ZtViewer *viewer);
/* One step of param on band 0-2 (dir -1/1, coarse 0/1); 1 when it changed. */
int32_t zt_viewer_eq_adjust(ZtViewer *viewer, int32_t band, int32_t param, int32_t dir,
                            int32_t coarse);
/* band 0-2 back to the M8's default; 1 when it changed. */
int32_t zt_viewer_eq_reset(ZtViewer *viewer, int32_t band);
/* Response in dB at n log-spaced points from 20 Hz to 20 kHz, of band 0-2
 * or (band -1) of the three together. */
void zt_eq_curve(const ZtEqState *state, int32_t band, float *out, size_t n);

/* 1 when the song has unsaved edits. */
int32_t zt_viewer_dirty(const ZtViewer *viewer);

/* Top-bar text: song name and tempo, NUL-terminated into out (len bytes). */
void zt_viewer_header(const ZtViewer *viewer, char *out, size_t len);

#ifdef __cplusplus
}
#endif

#endif
