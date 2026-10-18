/* C API of the ZeroTracker file format library (m8fmt). */
#ifndef ZT_FMT_H
#define ZT_FMT_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct ZtFileVersion {
    uint8_t major;
    uint8_t minor;
    uint8_t patch;
} ZtFileVersion;

/* Returns 1 and fills *out when data starts with an M8 version header, else 0. */
int32_t zt_fmt_read_version(const uint8_t *data, size_t len, ZtFileVersion *out);

/* Version of the core library, static NUL-terminated string. */
const char *zt_core_version(void);

#ifdef __cplusplus
}
#endif

#endif
