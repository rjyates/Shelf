/* Room-temperature panel busy times. emulator/viewer/damage.js uses the same numbers.
 * shortcut: fast is under 1/8 of the panel and inside the quality busy time,
 * and partials become full after 4 panel-areas, move when waveform files land.
 */
#ifndef OSEINK_REFRESH_H
#define OSEINK_REFRESH_H

#include <stdint.h>

#define OSEINK_BUSY_FAST_MS 120
#define OSEINK_BUSY_QUALITY_MS 450
#define OSEINK_BUSY_FULL_MS 2000
#define OSEINK_PARTIAL_BUDGET 4
#define OSEINK_FAST_RECT_DIV 8

struct oseink_refresh {
	int panel_w;
	int panel_h;
	long budget_px;
	long last_ms;
	uint8_t *gray;
};

void oseink_refresh_init(struct oseink_refresh *st, int panel_w, int panel_h);
void oseink_refresh_fini(struct oseink_refresh *st);

/* Updates *budget_px. Returns "fast", "quality", or "full". */
const char *oseink_mode(int w, int h, int panel_w, int panel_h, int dt_ms,
			long *budget_px);
int oseink_busy_ms(const char *mode);

/* Gray the damage rect (the whole panel when the mode is full) into a v1
 * gray8 frame under dir. Returns the busy time, or -1 if nothing was written.
 */
int oseink_refresh_publish(struct oseink_refresh *st, const uint32_t *xrgb,
			   int stride_px, int x, int y, int w, int h,
			   long now_ms, const char *dir);

#endif
