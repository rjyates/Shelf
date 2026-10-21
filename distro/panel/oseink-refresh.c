#ifdef OSEINK_REFRESH_MAIN
#define _GNU_SOURCE
#endif

#include "oseink-refresh.h"

#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static uint8_t
gray_xrgb(uint32_t pixel)
{
	unsigned r = (pixel >> 16) & 0xff;
	unsigned g = (pixel >> 8) & 0xff;
	unsigned b = pixel & 0xff;

	return (uint8_t)((r * 77 + g * 150 + b * 29) >> 8);
}

void
oseink_refresh_init(struct oseink_refresh *st, int panel_w, int panel_h)
{
	size_t n;

	free(st->gray);
	memset(st, 0, sizeof *st);
	st->panel_w = panel_w;
	st->panel_h = panel_h;
	st->last_ms = -1;
	if (panel_w <= 0 || panel_h <= 0)
		return;
	st->budget_px = (long)OSEINK_PARTIAL_BUDGET * (long)panel_w * (long)panel_h;
	n = (size_t)panel_w * (size_t)panel_h;
	st->gray = malloc(n);
	if (st->gray)
		memset(st->gray, 0xff, n);
}

void
oseink_refresh_fini(struct oseink_refresh *st)
{
	free(st->gray);
	st->gray = NULL;
}

const char *
oseink_mode(int w, int h, int panel_w, int panel_h, int dt_ms, long *budget_px,
	    const char *prev)
{
	long area = (long)w * (long)h;
	long panel = (long)panel_w * (long)panel_h;

	if (panel < 1)
		panel = 1;
	if (*budget_px <= 0) {
		*budget_px = (long)OSEINK_PARTIAL_BUDGET * panel;
		return "full";
	}
	/* Weston often damages the whole output. Only the first of those is a clear. */
	if (area >= panel) {
		if (!prev) {
			*budget_px = (long)OSEINK_PARTIAL_BUDGET * panel;
			return "full";
		}
		return "quality";
	}
	*budget_px -= area;
	/* A small rect before a quality update would have finished is typing. */
	if (dt_ms < OSEINK_BUSY_QUALITY_MS &&
	    area * OSEINK_FAST_RECT_DIV < panel)
		return "fast";
	return "quality";
}

int
oseink_busy_ms(const char *mode)
{
	if (strcmp(mode, "fast") == 0)
		return OSEINK_BUSY_FAST_MS;
	if (strcmp(mode, "quality") == 0)
		return OSEINK_BUSY_QUALITY_MS;
	if (strcmp(mode, "full") == 0)
		return OSEINK_BUSY_FULL_MS;
	return -1;
}

static int
join_path(char *out, size_t out_len, const char *dir, const char *name)
{
	int n = snprintf(out, out_len, "%s/%s", dir, name);

	if (n < 0 || (size_t)n >= out_len)
		return -1;
	return 0;
}

static int
write_atomic(const char *path, const void *data, size_t len)
{
	char tmp[512];
	int fd;
	int n;
	const uint8_t *bytes = data;
	size_t left = len;

	n = snprintf(tmp, sizeof tmp, "%s.tmp", path);
	if (n < 0 || (size_t)n >= sizeof tmp)
		return -1;
	fd = open(tmp, O_WRONLY | O_CREAT | O_TRUNC, 0644);
	if (fd < 0)
		return -1;
	while (left > 0) {
		ssize_t wrote = write(fd, bytes, left);

		if (wrote < 0) {
			if (errno == EINTR)
				continue;
			close(fd);
			unlink(tmp);
			return -1;
		}
		bytes += wrote;
		left -= (size_t)wrote;
	}
	if (close(fd) != 0) {
		unlink(tmp);
		return -1;
	}
	if (rename(tmp, path) != 0) {
		unlink(tmp);
		return -1;
	}
	return 0;
}

static int
next_seq(const char *dir)
{
	char path[512];
	FILE *file;
	int seq = 0;
	char text[32];
	int n;

	if (join_path(path, sizeof path, dir, "damage.seq") < 0)
		return -1;
	file = fopen(path, "r");
	if (file) {
		if (fscanf(file, "%d", &seq) != 1 || seq < 0)
			seq = 0;
		fclose(file);
	}
	seq++;
	n = snprintf(text, sizeof text, "%d\n", seq);
	if (n < 0 || write_atomic(path, text, (size_t)n) < 0)
		return -1;
	return seq;
}

int
oseink_refresh_publish(struct oseink_refresh *st, const uint32_t *xrgb,
		       int stride_px, int x, int y, int w, int h,
		       long now_ms, const char *dir)
{
	int dt;
	const char *mode;
	int busy;
	int seq;
	int gray_x, gray_y, gray_w, gray_h;
	int gx, gy;
	char path[512];
	char json[320];
	int json_n;
	int saved_x, saved_y, saved_w, saved_h;

	if (!st->gray || !xrgb || !dir || stride_px < st->panel_w)
		return -1;
	if (x < 0) {
		w += x;
		x = 0;
	}
	if (y < 0) {
		h += y;
		y = 0;
	}
	if (x >= st->panel_w || y >= st->panel_h)
		return -1;
	if (x + w > st->panel_w)
		w = st->panel_w - x;
	if (y + h > st->panel_h)
		h = st->panel_h - y;
	if (w <= 0 || h <= 0)
		return -1;

	dt = st->last_ms < 0 ? 1000000 : (int)(now_ms - st->last_ms);
	if (dt < 0)
		dt = 0;
	saved_x = x;
	saved_y = y;
	saved_w = w;
	saved_h = h;
	mode = oseink_mode(w, h, st->panel_w, st->panel_h, dt, &st->budget_px,
			   st->last_mode);
	busy = oseink_busy_ms(mode);
	if (busy < 0)
		return -1;

	if (strcmp(mode, "full") == 0) {
		gray_x = 0;
		gray_y = 0;
		gray_w = st->panel_w;
		gray_h = st->panel_h;
	} else {
		gray_x = x;
		gray_y = y;
		gray_w = w;
		gray_h = h;
	}
	for (gy = gray_y; gy < gray_y + gray_h; gy++) {
		for (gx = gray_x; gx < gray_x + gray_w; gx++)
			st->gray[(size_t)gy * (size_t)st->panel_w + (size_t)gx] =
				gray_xrgb(xrgb[(size_t)gy * (size_t)stride_px + (size_t)gx]);
	}

	if (join_path(path, sizeof path, dir, "frame.gray") < 0)
		return -1;
	if (write_atomic(path, st->gray,
			 (size_t)st->panel_w * (size_t)st->panel_h) < 0)
		return -1;
	seq = next_seq(dir);
	if (seq < 0)
		return -1;
	json_n = snprintf(json, sizeof json,
			  "{\"v\":1,\"mode\":\"%s\",\"x\":%d,\"y\":%d,\"w\":%d,\"h\":%d,"
			  "\"panel_w\":%d,\"panel_h\":%d,\"seq\":%d,\"format\":\"gray8\","
			  "\"pixels\":\"frame.gray\"}\n",
			  mode, saved_x, saved_y, saved_w, saved_h, st->panel_w,
			  st->panel_h, seq);
	if (json_n < 0 || (size_t)json_n >= sizeof json)
		return -1;
	if (join_path(path, sizeof path, dir, "damage.json") < 0)
		return -1;
	if (write_atomic(path, json, (size_t)json_n) < 0)
		return -1;
	st->last_ms = now_ms;
	st->last_mode = mode;
	return busy;
}

#ifdef OSEINK_REFRESH_MAIN
static int
fail(const char *msg)
{
	fprintf(stderr, "FAIL: %s\n", msg);
	return 1;
}

int
main(void)
{
	long budget;
	struct oseink_refresh st;
	uint32_t pixels[64];
	char dir[] = "/tmp/oseink-refresh-XXXXXX";
	char path[512];
	FILE *file;
	char json[320];
	int busy;
	int i;

	budget = (long)OSEINK_PARTIAL_BUDGET * 400 * 300;
	if (strcmp(oseink_mode(400, 300, 400, 300, 10, &budget, NULL), "full") != 0)
		return fail("full panel");
	if (budget != (long)OSEINK_PARTIAL_BUDGET * 400 * 300)
		return fail("full resets budget");
	budget = (long)OSEINK_PARTIAL_BUDGET * 400 * 300;
	if (strcmp(oseink_mode(400, 300, 400, 300, 10, &budget, "full"),
		   "quality") != 0)
		return fail("later covering frame");
	if (budget != (long)OSEINK_PARTIAL_BUDGET * 400 * 300)
		return fail("covering frame keeps the budget");

	budget = (long)OSEINK_PARTIAL_BUDGET * 400 * 300;
	if (strcmp(oseink_mode(8, 8, 400, 300, 10, &budget, NULL), "fast") != 0)
		return fail("small recent rect");
	if (strcmp(oseink_mode(200, 200, 400, 300, 10, &budget, NULL), "quality") != 0)
		return fail("large partial");
	budget = (long)OSEINK_PARTIAL_BUDGET * 400 * 300;
	if (strcmp(oseink_mode(8, 8, 400, 300, OSEINK_BUSY_QUALITY_MS, &budget, NULL),
		   "quality") != 0)
		return fail("small rect after the quality interval");
	budget = 0;
	if (strcmp(oseink_mode(8, 8, 400, 300, 10, &budget, NULL), "full") != 0)
		return fail("spent budget");
	if (oseink_busy_ms("fast") != 120 || oseink_busy_ms("quality") != 450 ||
	    oseink_busy_ms("full") != 2000)
		return fail("busy times");
	if (!(oseink_busy_ms("full") > oseink_busy_ms("quality") &&
	      oseink_busy_ms("quality") > oseink_busy_ms("fast")))
		return fail("full is the slow clear");

	if (!mkdtemp(dir))
		return fail("tmpdir");
	memset(&st, 0, sizeof st);
	oseink_refresh_init(&st, 8, 8);
	for (i = 0; i < 64; i++)
		pixels[i] = 0x00ffffff;
	busy = oseink_refresh_publish(&st, pixels, 8, 0, 0, 8, 8, 1000, dir);
	if (busy != 2000)
		return fail("full-panel publish busy");
	pixels[0] = 0x00000000;
	busy = oseink_refresh_publish(&st, pixels, 8, 0, 0, 1, 1, 1010, dir);
	if (busy != 120)
		return fail("small publish busy");
	if (st.gray[0] != 0 || st.gray[1] != 255)
		return fail("damage rect grayed");
	snprintf(path, sizeof path, "%s/damage.json", dir);
	file = fopen(path, "r");
	if (!file || !fgets(json, sizeof json, file))
		return fail("damage.json");
	fclose(file);
	if (!strstr(json, "\"mode\":\"fast\"") || !strstr(json, "\"format\":\"gray8\"") ||
	    !strstr(json, "\"seq\":2") || strstr(json, "color"))
		return fail("v1 gray8 message");
	oseink_refresh_fini(&st);
	printf("ok: mode from rect, time, and budget\n");
	printf("ok: gray8 frame\n");
	return 0;
}
#endif
