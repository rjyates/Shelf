/* Profile evdev devices for the desktop seat.
 * Lines in DIR, one event each, names 00000001 and up:
 *   k CODE 0|1
 *   t X Y 0|1
 *   s X Y 0|1 PRESSURE
 * A name that is not all digits is ignored. The file is removed after it is read.
 */
#include <ctype.h>
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <linux/input.h>
#include <linux/uinput.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#define PRESSURE_MAX 4095

static int width_max = 1;
static int height_max = 1;
static int key_fd = -1;
static int touch_fd = -1;
static int stylus_fd = -1;
static int touch_down;
static int stylus_down;
static int tracking = 1;

static void die(const char *what) {
	fprintf(stderr, "oseink-input: %s: %s\n", what, strerror(errno));
	exit(1);
}

static void emit(int fd, int type, int code, int value) {
	struct input_event ev;

	memset(&ev, 0, sizeof ev);
	ev.type = type;
	ev.code = code;
	ev.value = value;
	if (write(fd, &ev, sizeof ev) != (ssize_t)sizeof ev)
		die("write");
}

static void syn(int fd) {
	emit(fd, EV_SYN, SYN_REPORT, 0);
}

static int axis(int fd, int code, int min, int max, int res) {
	struct uinput_abs_setup abs;

	if (ioctl(fd, UI_SET_ABSBIT, code) < 0)
		return -1;
	memset(&abs, 0, sizeof abs);
	abs.code = code;
	abs.absinfo.minimum = min;
	abs.absinfo.maximum = max;
	abs.absinfo.resolution = res > 0 ? res : 1;
	return ioctl(fd, UI_ABS_SETUP, &abs);
}

static int finish(int fd, const char *name) {
	struct uinput_setup setup;

	memset(&setup, 0, sizeof setup);
	setup.id.bustype = BUS_VIRTUAL;
	setup.id.vendor = 0x1111;
	setup.id.product = 0x26;
	snprintf(setup.name, sizeof setup.name, "%s", name);
	if (ioctl(fd, UI_DEV_SETUP, &setup) < 0)
		return -1;
	return ioctl(fd, UI_DEV_CREATE, 0);
}

static int open_dev(void) {
	int fd = open("/dev/uinput", O_WRONLY | O_NONBLOCK);
	if (fd < 0)
		die("open /dev/uinput");
	return fd;
}

static void add_keys(int fd) {
	int code;

	if (ioctl(fd, UI_SET_EVBIT, EV_KEY) < 0)
		die("key");
	/* Keys 1..31 make udev call this a keyboard. Space, slash, and shift sit above that. */
	for (code = 1; code <= KEY_SPACE; code++) {
		if (ioctl(fd, UI_SET_KEYBIT, code) < 0)
			die("keybit");
	}
}

static void add_keyboard(void) {
	key_fd = open_dev();
	add_keys(key_fd);
	if (finish(key_fd, "oseink buttons") < 0)
		die("buttons");
}

static void add_pointer_axes(int fd, int res) {
	if (ioctl(fd, UI_SET_EVBIT, EV_ABS) < 0)
		die("abs");
	if (ioctl(fd, UI_SET_PROPBIT, INPUT_PROP_DIRECT) < 0)
		die("prop");
	if (axis(fd, ABS_X, 0, width_max, res) < 0 || axis(fd, ABS_Y, 0, height_max, res) < 0)
		die("axis");
}

static void add_touch(int res) {
	touch_fd = open_dev();
	add_pointer_axes(touch_fd, res);
	if (axis(touch_fd, ABS_MT_SLOT, 0, 0, 0) < 0 ||
	    axis(touch_fd, ABS_MT_TRACKING_ID, -1, 65535, 0) < 0 ||
	    axis(touch_fd, ABS_MT_POSITION_X, 0, width_max, res) < 0 ||
	    axis(touch_fd, ABS_MT_POSITION_Y, 0, height_max, res) < 0)
		die("mt");
	if (finish(touch_fd, "oseink touch") < 0)
		die("touch");
}

static void add_stylus(int res) {
	stylus_fd = open_dev();
	add_pointer_axes(stylus_fd, res);
	if (ioctl(stylus_fd, UI_SET_EVBIT, EV_KEY) < 0)
		die("pen");
	if (ioctl(stylus_fd, UI_SET_KEYBIT, BTN_TOUCH) < 0 ||
	    ioctl(stylus_fd, UI_SET_KEYBIT, BTN_TOOL_PEN) < 0)
		die("pen");
	if (axis(stylus_fd, ABS_PRESSURE, 0, PRESSURE_MAX, 0) < 0)
		die("pressure");
	if (finish(stylus_fd, "oseink stylus") < 0)
		die("stylus");
}

static int clamp(int value, int max) {
	if (value < 0)
		return 0;
	if (value > max)
		return max;
	return value;
}

static void send_key(int code, int down) {
	if (key_fd < 0 || code < 1 || code > KEY_SPACE || (down != 0 && down != 1))
		return;
	emit(key_fd, EV_KEY, code, down);
	syn(key_fd);
}

static void send_touch(int x, int y, int down) {
	if (touch_fd < 0 || (down != 0 && down != 1))
		return;
	x = clamp(x, width_max);
	y = clamp(y, height_max);
	emit(touch_fd, EV_ABS, ABS_MT_SLOT, 0);
	if (down && !touch_down) {
		emit(touch_fd, EV_ABS, ABS_MT_TRACKING_ID, tracking++);
		touch_down = 1;
	}
	emit(touch_fd, EV_ABS, ABS_MT_POSITION_X, x);
	emit(touch_fd, EV_ABS, ABS_MT_POSITION_Y, y);
	emit(touch_fd, EV_ABS, ABS_X, x);
	emit(touch_fd, EV_ABS, ABS_Y, y);
	if (!down && touch_down) {
		emit(touch_fd, EV_ABS, ABS_MT_TRACKING_ID, -1);
		touch_down = 0;
	}
	syn(touch_fd);
}

static void send_stylus(int x, int y, int down, int pressure) {
	if (stylus_fd < 0 || (down != 0 && down != 1))
		return;
	x = clamp(x, width_max);
	y = clamp(y, height_max);
	if (pressure < 0)
		pressure = 0;
	if (pressure > PRESSURE_MAX)
		pressure = PRESSURE_MAX;
	if (down && !stylus_down) {
		emit(stylus_fd, EV_KEY, BTN_TOOL_PEN, 1);
		stylus_down = 1;
	}
	emit(stylus_fd, EV_ABS, ABS_X, x);
	emit(stylus_fd, EV_ABS, ABS_Y, y);
	emit(stylus_fd, EV_ABS, ABS_PRESSURE, down ? pressure : 0);
	emit(stylus_fd, EV_KEY, BTN_TOUCH, down);
	if (!down && stylus_down) {
		emit(stylus_fd, EV_KEY, BTN_TOOL_PEN, 0);
		stylus_down = 0;
	}
	syn(stylus_fd);
}

static void handle(char *line) {
	int a, b, c, d;

	if (sscanf(line, "k %d %d", &a, &b) == 2)
		send_key(a, b);
	else if (sscanf(line, "t %d %d %d", &a, &b, &c) == 3)
		send_touch(a, b, c);
	else if (sscanf(line, "s %d %d %d %d", &a, &b, &c, &d) == 4)
		send_stylus(a, b, c, d);
}

static int all_digits(const char *name) {
	if (!*name)
		return 0;
	for (; *name; name++) {
		if (!isdigit((unsigned char)*name))
			return 0;
	}
	return 1;
}

static int cmp_names(const void *a, const void *b) {
	return strcmp(*(char *const *)a, *(char *const *)b);
}

static unsigned seen_sig;
static int seen_polls;

/* Wait until the queued names sit still, then read them in order.
 * One poll is not enough: a key-down file shows up before its key-up, and the
 * terminal autorepeats the held key. A burst also arrives a few files at a
 * time on 9p, and sorting a partial list types the later keys first. */
static void drain(const char *dir) {
	DIR *dp;
	struct dirent *ent;
	char *names[256];
	int n = 0;
	int i;
	unsigned sig = 2166136261u;

	dp = opendir(dir);
	if (!dp)
		return;
	while ((ent = readdir(dp)) != NULL && n < 256) {
		if (!all_digits(ent->d_name))
			continue;
		names[n] = strdup(ent->d_name);
		if (!names[n])
			die("strdup");
		n++;
	}
	closedir(dp);
	qsort(names, (size_t)n, sizeof names[0], cmp_names);
	for (i = 0; i < n; i++) {
		const char *p;
		sig ^= (unsigned)n;
		for (p = names[i]; *p; p++)
			sig = (sig ^ (unsigned char)*p) * 16777619u;
	}
	if (n == 0 || sig != seen_sig) {
		seen_sig = n == 0 ? 0 : sig;
		seen_polls = n == 0 ? 0 : 1;
		for (i = 0; i < n; i++)
			free(names[i]);
		return;
	}
	if (++seen_polls < 4) {
		for (i = 0; i < n; i++)
			free(names[i]);
		return;
	}
	seen_sig = 0;
	seen_polls = 0;
	for (i = 0; i < n; i++) {
		char path[512];
		char buf[128];
		FILE *fp;

		snprintf(path, sizeof path, "%s/%s", dir, names[i]);
		fp = fopen(path, "r");
		if (fp) {
			if (fgets(buf, sizeof buf, fp))
				handle(buf);
			fclose(fp);
		}
		unlink(path);
		free(names[i]);
		usleep(4000);
	}
}

int main(int argc, char **argv) {
	const char *dir;
	int dpi;
	int i;
	int want_key = 0;
	int want_touch = 0;
	int want_stylus = 0;

	if (argc < 5) {
		fprintf(stderr, "usage: oseink-input DIR WIDTH HEIGHT DPI [buttons|touch|stylus]...\n");
		return 2;
	}
	dir = argv[1];
	width_max = atoi(argv[2]);
	height_max = atoi(argv[3]);
	dpi = atoi(argv[4]);
	if (width_max < 1 || height_max < 1)
		return 2;
	width_max -= 1;
	height_max -= 1;
	if (width_max < 1)
		width_max = 1;
	if (height_max < 1)
		height_max = 1;
	for (i = 5; i < argc; i++) {
		if (strcmp(argv[i], "buttons") == 0)
			want_key = 1;
		else if (strcmp(argv[i], "touch") == 0)
			want_touch = 1;
		else if (strcmp(argv[i], "stylus") == 0)
			want_stylus = 1;
		else
			return 2;
	}
	if (!want_key && !want_touch && !want_stylus)
		return 2;
	if (mkdir(dir, 0777) < 0 && errno != EEXIST)
		die("mkdir");
	/* The viewer may have created this directory as root. The host test writes events here. */
	chmod(dir, 0777);
	if (want_key)
		add_keyboard();
	if (want_touch)
		add_touch(dpi);
	if (want_stylus)
		add_stylus(dpi);
	for (;;) {
		drain(dir);
		usleep(10000);
	}
}
