// SPDX-License-Identifier: GPL-2.0-only
/*
 * clevo-cc - ACPI platform driver for Clevo DCHU devices.
 *
 * This is slice S6a: it binds ACPI\CLV0001, evaluates the firmware `_DSM`
 * method using a real ACPI Package argument (which acpi_call cannot build),
 * and exposes the verified read commands through hwmon:
 *
 *   fan1_input  - CPU fan speed in rpm
 *   fan2_input  - GPU1 fan speed in rpm
 *
 * Read commands use Arg3 = Package { Buffer(256) }; verified command-121
 * scalar writes use Package { Integer(value) }, while command-103 RGB writes
 * use Package { Buffer(256) } to match the vendor's byte-array call.
 *
 * Verified against a COLORFUL P15 23; see docs/hardware-notes.md.
 */

#include <linux/acpi.h>
#include <linux/delay.h>
#include <linux/hwmon.h>
#include <linux/hwmon-sysfs.h>
#include <linux/leds.h>
#include <linux/module.h>
#include <linux/mutex.h>
#include <linux/platform_device.h>

#define CLEVO_DSM_GUID "93F224E4-FBDC-4BBF-ADD6-DB71BDC0AFAD"
#define CLEVO_PAYLOAD_LEN 256

/*
 * Shortest command-12 reply that carries the verified fields: the temperature
 * triple ends at offset 21. The live firmware answers with 42 bytes.
 */
#define CLEVO_FAN_STATUS_MIN_LEN 22

/* Command numbers (verified; see clevo-proto). */
#define CLEVO_CMD_FAN_STATUS 12
#define CLEVO_CMD_FAN_CURVE 13
#define CLEVO_CMD_FAN_CURVE_WRITE 14
#define CLEVO_CMD_MAIN 121
#define CLEVO_CMD_KEYBOARD_RGB 103

/*
 * RGB15 command-103 selector used by the COLORFUL P15 23.  The vendor
 * utility contains F0/F1/F2 paths for models with multiple zones, but this
 * machine's kb_type=6 firmware only has one physical RGB channel.  F1/F2 are
 * accepted by ACPI yet do not address any LEDs, which made the old interface
 * look like it was changing a zone while the whole keyboard stayed on F0.
 */
#define CLEVO_KB_ZONE_SINGLE 0xF0
#define CLEVO_KB_ZONE_MIDDLE 0xF1
#define CLEVO_KB_ZONE_RIGHT 0xF2
#define CLEVO_KB_BRIGHTNESS_BASE 0xF4000000
#define CLEVO_KB_BRIGHTNESS_MAX 191
/* Percent scale exposed by the named keyboard_rgb interface (0..100). */
#define CLEVO_KB_BRIGHTNESS_PERCENT_MAX 100
#define CLEVO_KB_COLOR_FADE_STEPS 24
#define CLEVO_KB_COLOR_FADE_DELAY_US 10000
/*
 * Native RGB15 effect words from the vendor's RGBKB.SetMode(), which is the
 * class the firmware selects for kb_type 6/22. `static` has no effect word: it
 * is just the persisted per-channel colors.
 */
#define CLEVO_KB_RANDOM 0x70000000
#define CLEVO_KB_DANCE 0x80000000
#define CLEVO_KB_TEMPO 0x90000000
#define CLEVO_KB_FLASH 0xA0000000
#define CLEVO_KB_WAVE 0xB0000000
#define CLEVO_KB_BREATH 0x1002A000
#define CLEVO_KB_CYCLE 0x33010000
/* Vendor RGB15 status word with the available keyboard channel enabled. */
#define CLEVO_KB_STATUS_ON 0xE0071007
#define CLEVO_KB_STATUS_OFF 0xE0000007

/* CLEVO_CMD_MAIN sub-command for fan mode. */
#define CLEVO_SUB_FAN_MODE 1

/* CLEVO_CMD_MAIN sub-command used by RGBKB to control the LED sleep timer. */
#define CLEVO_SUB_KB_SLEEP_TIMER 24

/* CLEVO_CMD_MAIN sub-command for performance mode (0..3). */
#define CLEVO_SUB_PERF_MODE 25

/* Fan mode values accepted by CLEVO_SUB_FAN_MODE (DCHU 121/1), matching the
 * original Control Center fan page buttons:
 *   0 = auto, 1 = maximum, 3 = silent, 5 = max-q, 6 = custom, 8 = slow/quiet
 *
 * On this firmware `silent` (3) is an empty branch in the DSDT and `custom`
 * (6) only selects "use the custom curve", which has no effect until a curve
 * is written with command 14.
 */
#define CLEVO_FAN_MODE_AUTO 0
#define CLEVO_FAN_MODE_MAXIMUM 1
#define CLEVO_FAN_MODE_MAXQ 5
#define CLEVO_FAN_MODE_CUSTOM 6
#define CLEVO_FAN_MODE_QUIET 8

/* Driver-visible fan modes. */
enum clevo_fan_mode {
	CLEVO_MODE_AUTO = 0,
	CLEVO_MODE_QUIET,
	CLEVO_MODE_MAX,
	CLEVO_MODE_MAXQ,
	CLEVO_MODE_CUSTOM,
};

/* Driver-visible performance modes (DCHU 121/25 values). */
enum clevo_perf_mode {
	CLEVO_PERF_QUIET = 0,
	CLEVO_PERF_PWRSAVING = 1,
	CLEVO_PERF_PERFORMANCE = 2,
	CLEVO_PERF_ENTERTAINMENT = 3,
};

struct clevo_cc {
	struct acpi_device *adev;
	u8 dsm_guid[16];
	enum clevo_fan_mode fan_mode;
	enum clevo_perf_mode perf_mode;
	bool perf_mode_set;
	struct mutex keyboard_lock;
	struct led_classdev keyboard_led;
	u8 keyboard_color[3][3];
	bool keyboard_color_known[3];
	/* keyboard_rgb keeps a 0..100 percentage; LED class uses the raw byte. */
	u8 keyboard_brightness;
	u8 keyboard_brightness_raw;
	const char *keyboard_mode;
};

/*
 * Interpret the integer a `_DSM` call returned.
 *
 * Success codes differ by command family, and both were verified live:
 *
 *   - SCMD/GCMD (e.g. 121) return the command number itself (`0x79`).
 *   - The fan-curve write (14) returns `0x14` (20), documented in
 *     docs/hardware-notes.md §7.
 *   - `0x80000002` means "not supported".
 *
 * Treating only `ret == function` as success made a working curve write look
 * like a failure: the EC accepted the curve and returned 20, which is exactly
 * what §7 said it would.
 */
static int clevo_cc_dsm_status(u32 function, u64 value)
{
	if (value == function)
		return 0;

	/* Command 14 reports success as 20 (0x14). */
	if (function == CLEVO_CMD_FAN_CURVE_WRITE && value == 20)
		return 0;

	if (value == 0x80000002)
		return -EOPNOTSUPP;

	return -EIO;
}

/*
 * Evaluate the firmware `_DSM` method:
 *   _DSM(GUID buffer, Revision=0, Function, Arg3)
 *
 * `arg3` is taken over by this call (its dynamically allocated buffer is freed
 * here). Returns -errno, or 0 with `out_len` bytes copied to `out`.
 */
static int clevo_cc_dsm_call(struct clevo_cc *cc, u32 function,
			     union acpi_object *arg3, u8 *out, size_t out_cap,
			     size_t *out_len)
{
	struct acpi_object_list args;
	union acpi_object argv[4];
	union acpi_object *ret;
	struct acpi_buffer buf = { ACPI_ALLOCATE_BUFFER, NULL };
	acpi_status status;
	int err = 0;

	/* Argument 0: the method GUID as a 16-byte buffer. */
	argv[0].type = ACPI_TYPE_BUFFER;
	argv[0].buffer.length = sizeof(cc->dsm_guid);
	argv[0].buffer.pointer = cc->dsm_guid;
	/* Argument 1: revision. */
	argv[1].type = ACPI_TYPE_INTEGER;
	argv[1].integer.value = 0;
	/* Argument 2: function index (command). */
	argv[2].type = ACPI_TYPE_INTEGER;
	argv[2].integer.value = function;
	/* Argument 3: caller-provided payload object. */
	argv[3] = *arg3;

	args.count = 4;
	args.pointer = argv;

	status = acpi_evaluate_object(cc->adev->handle, "_DSM", &args, &buf);
	if (ACPI_FAILURE(status)) {
		dev_warn(&cc->adev->dev,
			 "_DSM function %u failed: %s (arg3 type %u)\n",
			 function, acpi_format_exception(status), arg3->type);
		err = -EIO;
		goto out;
	}

	ret = buf.pointer;
	if (ret->type == ACPI_TYPE_BUFFER) {
		size_t n = min(out_cap, (size_t)ret->buffer.length);

		memcpy(out, ret->buffer.pointer, n);
		*out_len = n;
	} else if (ret->type == ACPI_TYPE_INTEGER) {
		err = clevo_cc_dsm_status(function, ret->integer.value);
		if (err == -EOPNOTSUPP)
			dev_warn(&cc->adev->dev,
				 "_DSM function %u returned 0x80000002 (unsupported)\n",
				 function);
		else if (err)
			dev_warn(&cc->adev->dev,
				 "_DSM function %u returned unexpected integer 0x%llx\n",
				 function, ret->integer.value);
	} else {
		dev_warn(&cc->adev->dev, "_DSM function %u returned type %u\n",
			 function, ret->type);
		err = -EIO;
	}

out:
	kfree(buf.pointer);
	return err;
}

/* Build Arg3 as Package { Buffer(256) }, for read commands (class PK*). */
static int clevo_cc_dsm_payload(struct clevo_cc *cc, u32 function, const u8 *in,
				u8 *out, size_t out_cap, size_t *out_len);

static int clevo_cc_dsm_buffer(struct clevo_cc *cc, u32 function, u8 *out,
			       size_t out_cap, size_t *out_len)
{
	return clevo_cc_dsm_payload(cc, function, NULL, out, out_cap, out_len);
}

/*
 * Like clevo_cc_dsm_buffer, but sends `in` (256 bytes) as the payload instead
 * of zeros. Used by command 14, which carries the curve in the buffer.
 */
static int clevo_cc_dsm_payload(struct clevo_cc *cc, u32 function, const u8 *in,
				u8 *out, size_t out_cap, size_t *out_len)
{
	union acpi_object pkg;
	u8 *payload;
	int err;

	payload = kzalloc(CLEVO_PAYLOAD_LEN, GFP_KERNEL);
	if (!payload)
		return -ENOMEM;
	if (in)
		memcpy(payload, in, CLEVO_PAYLOAD_LEN);

	pkg.type = ACPI_TYPE_PACKAGE;
	pkg.package.count = 1;
	pkg.package.elements = kcalloc(1, sizeof(union acpi_object), GFP_KERNEL);
	if (!pkg.package.elements) {
		kfree(payload);
		return -ENOMEM;
	}
	pkg.package.elements[0].type = ACPI_TYPE_BUFFER;
	pkg.package.elements[0].buffer.length = CLEVO_PAYLOAD_LEN;
	pkg.package.elements[0].buffer.pointer = payload;

	err = clevo_cc_dsm_call(cc, function, &pkg, out, out_cap, out_len);

	kfree(pkg.package.elements);
	kfree(payload);
	return err;
}

/*
 * Build Arg3 as Package { Integer(value) }.
 *
 * SCMD (command 121) does `ARGS = Arg2` where ARGS is an Integer, and also
 * uses Index() on the argument, so it wants a Package whose first element is
 * the scalar. (A bare Integer fails with AE_AML_OPERAND_TYPE at Index, and a
 * large Buffer cannot be converted to an Integer.)
 */
static int clevo_cc_dsm_scalar(struct clevo_cc *cc, u32 function, u32 value,
				       u8 *out, size_t out_cap, size_t *out_len)
{
	union acpi_object pkg;
	int err;

	pkg.type = ACPI_TYPE_PACKAGE;
	pkg.package.count = 1;
	pkg.package.elements = kcalloc(1, sizeof(union acpi_object), GFP_KERNEL);
	if (!pkg.package.elements)
		return -ENOMEM;
	pkg.package.elements[0].type = ACPI_TYPE_INTEGER;
	pkg.package.elements[0].integer.value = value;

	err = clevo_cc_dsm_call(cc, function, &pkg, out, out_cap, out_len);
	kfree(pkg.package.elements);
	return err;
}

/*
 * Send a CLEVO_CMD_MAIN sub-command with a scalar value.
 *
 * The DCHU encoding packs the sub-command in the high byte and the value in
 * the low 24 bits.
 */
static int clevo_cc_main_cmd(struct clevo_cc *cc, u32 sub, u32 value)
{
	u32 arg = (sub << 24) | (value & 0x00FFFFFF);
	u8 out[8];
	size_t len = 0;

	return clevo_cc_dsm_scalar(cc, CLEVO_CMD_MAIN, arg, out, sizeof(out),
				   &len);
}

/*
 * Set the fan mode via command 121 sub-command 1.
 */
static int clevo_cc_set_fan_mode(struct clevo_cc *cc, u32 mode)
{
	return clevo_cc_main_cmd(cc, CLEVO_SUB_FAN_MODE, mode);
}

/*
 * Set the performance mode via command 121 sub-command 25 (values 0..3).
 *
 * The firmware gates this on the PSF4 capability bit and rejects unsupported
 * values with 0x80000002, which the caller surfaces as -EOPNOTSUPP. The
 * firmware also applies an internal APPM mapping to the EC; we only send the
 * logical mode value, matching the Control Center.
 */
static int clevo_cc_set_perf_mode(struct clevo_cc *cc, u32 mode)
{
	if (mode > CLEVO_PERF_ENTERTAINMENT)
		return -EINVAL;
	return clevo_cc_main_cmd(cc, CLEVO_SUB_PERF_MODE, mode);
}

/*
 * Send one of the vendor RGB15 command-103 words.
 *
 * Unlike command 121, the vendor sends command 103 as a Package containing
 * a 256-byte Buffer.  The first four bytes contain the little-endian word;
 * the remaining bytes are zero-filled by InsydeDCHU.dll.
 */
static int clevo_cc_keyboard_command(struct clevo_cc *cc, u32 value)
{
	u8 payload[CLEVO_PAYLOAD_LEN] = { 0 };
	u8 out[8];
	size_t len = 0;

	payload[0] = value & 0xff;
	payload[1] = (value >> 8) & 0xff;
	payload[2] = (value >> 16) & 0xff;
	payload[3] = (value >> 24) & 0xff;
	return clevo_cc_dsm_payload(cc, CLEVO_CMD_KEYBOARD_RGB, payload, out,
				    sizeof(out), &len);
}

/* Encode a COLORREF-like RGB value used by RGBKB.cs: B, R, G in the low word. */
static u32 clevo_cc_keyboard_color_word(u8 selector, const u8 color[3])
{
	u32 rgb = ((u32)color[2] << 16) | ((u32)color[0] << 8) | color[1];

	/* Vendor firmware reserves this RGB15 palette entry. */
	if (color[0] == 0 && color[1] == 255 && color[2] == 127)
		rgb = 0x460000 | ((u32)color[0] << 8) | color[1];

	return ((u32)selector << 24) | rgb;
}

static int clevo_cc_set_keyboard_color(struct clevo_cc *cc, int zone,
					       const u8 color[3])
{
	if (zone < 0 || zone >= 3)
		return -EINVAL;

	return clevo_cc_keyboard_command(
		cc, clevo_cc_keyboard_color_word(CLEVO_KB_ZONE_SINGLE, color));
}

/* Software fade for the single physical RGB15 channel. */
static int clevo_cc_transition_keyboard_color(struct clevo_cc *cc,
						 const u8 target[3])
{
	u8 from[3];
	u8 color[3];
	unsigned int step;

	if (!cc->keyboard_color_known[0])
		return clevo_cc_set_keyboard_color(cc, 0, target);

	memcpy(from, cc->keyboard_color[0], sizeof(from));
	for (step = 1; step <= CLEVO_KB_COLOR_FADE_STEPS; step++) {
		int component;
		int err;

		for (component = 0; component < 3; component++) {
			int delta = (int)target[component] - from[component];
			color[component] = from[component] +
				(delta * (int)step) / CLEVO_KB_COLOR_FADE_STEPS;
		}

		err = clevo_cc_set_keyboard_color(cc, 0, color);
		if (err)
			return err;
		if (step != CLEVO_KB_COLOR_FADE_STEPS)
			usleep_range(CLEVO_KB_COLOR_FADE_DELAY_US,
				     CLEVO_KB_COLOR_FADE_DELAY_US + 2000);
	}

	return 0;
}

/* Send a raw RGB15 selector for hardware probing; this never updates cache. */
static int clevo_cc_probe_keyboard_color(struct clevo_cc *cc, int zone,
						 const u8 color[3])
{
	u8 selector;

	switch (zone) {
	case 0:
		selector = CLEVO_KB_ZONE_SINGLE;
		break;
	case 1:
		selector = CLEVO_KB_ZONE_MIDDLE;
		break;
	case 2:
		selector = CLEVO_KB_ZONE_RIGHT;
		break;
	default:
		return -EINVAL;
	}

	return clevo_cc_keyboard_command(
		cc, clevo_cc_keyboard_color_word(selector, color));
}

static int clevo_cc_set_keyboard_brightness_raw(struct clevo_cc *cc, u8 raw)
{
	return clevo_cc_keyboard_command(cc,
					CLEVO_KB_BRIGHTNESS_BASE | raw);
}

/*
 * Map a 0..100 percentage to the RGB15 raw brightness byte (0..191).
 *
 * The channel is analog, so there is no need to quantise onto the vendor's five
 * calibrated steps: 100% means the EC maximum, and every value in between is
 * available. Rounding is nearest, so 50% -> 96 rather than truncating to 95.
 */
static u8 clevo_cc_keyboard_percent_raw(u8 percent)
{
	return (u8)(((unsigned int)percent * CLEVO_KB_BRIGHTNESS_MAX +
		     CLEVO_KB_BRIGHTNESS_PERCENT_MAX / 2) /
		    CLEVO_KB_BRIGHTNESS_PERCENT_MAX);
}

/* Inverse of clevo_cc_keyboard_percent_raw, for reporting the cached percent. */
static u8 clevo_cc_keyboard_raw_percent(u8 raw)
{
	return (u8)(((unsigned int)raw * CLEVO_KB_BRIGHTNESS_PERCENT_MAX +
		     CLEVO_KB_BRIGHTNESS_MAX / 2) /
		    CLEVO_KB_BRIGHTNESS_MAX);
}

static int clevo_cc_set_keyboard_brightness_percent(struct clevo_cc *cc, u8 percent)
{
	if (percent > CLEVO_KB_BRIGHTNESS_PERCENT_MAX)
		return -EINVAL;
	return clevo_cc_set_keyboard_brightness_raw(
		cc, clevo_cc_keyboard_percent_raw(percent));
}

static int clevo_cc_set_keyboard_status(struct clevo_cc *cc, bool on)
{
	return clevo_cc_keyboard_command(
		cc, on ? CLEVO_KB_STATUS_ON : CLEVO_KB_STATUS_OFF);
}

/* RGBKB.SetSleepTimerTriggerOff(): command 121/24 with value 0. */
static int clevo_cc_disable_keyboard_sleep_timer(struct clevo_cc *cc)
{
	return clevo_cc_main_cmd(cc, CLEVO_SUB_KB_SLEEP_TIMER, 0);
}

/* Standard LED-class bridge used by KDE/PowerDevil for keyboard brightness. */
static int clevo_cc_keyboard_led_set(struct led_classdev *led_cdev,
					     enum led_brightness brightness)
{
	struct clevo_cc *cc = container_of(led_cdev, struct clevo_cc,
					   keyboard_led);
	int err;

	if (brightness > CLEVO_KB_BRIGHTNESS_MAX)
		return -EINVAL;

	mutex_lock(&cc->keyboard_lock);
	if (brightness == LED_OFF) {
		err = clevo_cc_set_keyboard_brightness_percent(cc, 0);
		if (!err)
			err = clevo_cc_set_keyboard_status(cc, false);
	} else {
		err = clevo_cc_set_keyboard_status(cc, true);
		if (!err)
			err = clevo_cc_set_keyboard_brightness_raw(cc, brightness);
	}
	if (!err)
		err = clevo_cc_disable_keyboard_sleep_timer(cc);
	if (!err) {
		cc->keyboard_brightness_raw = brightness;
		cc->keyboard_brightness = clevo_cc_keyboard_raw_percent(brightness);
		cc->keyboard_led.brightness = brightness;
	}
	mutex_unlock(&cc->keyboard_lock);
	return err;
}

static enum led_brightness
clevo_cc_keyboard_led_get(struct led_classdev *led_cdev)
{
	struct clevo_cc *cc = container_of(led_cdev, struct clevo_cc,
					   keyboard_led);
	enum led_brightness brightness;

	mutex_lock(&cc->keyboard_lock);
	brightness = cc->keyboard_led.brightness;
	mutex_unlock(&cc->keyboard_lock);
	return brightness;
}

static const char *clevo_cc_keyboard_mode_name(const struct clevo_cc *cc)
{
	return cc->keyboard_mode ?: "unknown";
}

/*
 * Native RGB15 effects exposed by the named interfaces. `static` is not in the
 * table because it has no effect word; it re-applies the persisted colors.
 */
static const struct {
	const char *name;
	u32 word;
} clevo_cc_keyboard_effects[] = {
	{ "random", CLEVO_KB_RANDOM },
	{ "breath", CLEVO_KB_BREATH },
	{ "cycle",  CLEVO_KB_CYCLE },
	{ "wave",   CLEVO_KB_WAVE },
	{ "dance",  CLEVO_KB_DANCE },
	{ "tempo",  CLEVO_KB_TEMPO },
	{ "flash",  CLEVO_KB_FLASH },
};

static ssize_t keyboard_rgb_show(struct device *dev,
					struct device_attribute *attr, char *buf)
{
	struct clevo_cc *cc = dev_get_drvdata(dev);
	ssize_t n;

	mutex_lock(&cc->keyboard_lock);
	n = sysfs_emit(buf,
		       "mode=%s brightness=%u raw_brightness=%u zones=1 color=%02x%02x%02x\n",
		       clevo_cc_keyboard_mode_name(cc), cc->keyboard_brightness,
		       cc->keyboard_brightness_raw,
		       cc->keyboard_color[0][0], cc->keyboard_color[0][1],
		       cc->keyboard_color[0][2]);
	mutex_unlock(&cc->keyboard_lock);
	return n;
}

static int clevo_cc_parse_keyboard_color(const char *text, u8 color[3])
{
	unsigned int r, g, b;
	int consumed;

	if (strlen(text) != 6 ||
	    sscanf(text, "%2x%2x%2x%n", &r, &g, &b, &consumed) != 3 ||
	    text[consumed] != '\0' || r > 0xFF || g > 0xFF || b > 0xFF)
		return -EINVAL;
	color[0] = (u8)r;
	color[1] = (u8)g;
	color[2] = (u8)b;
	return 0;
}

/*
 * Re-arm the single physical channel before an effect or color write: enable
 * the LEDs, restore the cached brightness, re-apply the persisted color, push
 * the effect word (NULL for `static`) and stop the firmware sleep timer.
 */
static int clevo_cc_apply_keyboard_effect(struct clevo_cc *cc, const char *name,
						  u32 word)
{
	int zone;
	int err;

	err = clevo_cc_set_keyboard_status(cc, true);
	if (err)
		return err;
	err = clevo_cc_set_keyboard_brightness_raw(cc, cc->keyboard_brightness_raw);
	if (err)
		return err;
	for (zone = 0; zone < 1 && !err; zone++) {
		if (cc->keyboard_color_known[zone])
			err = clevo_cc_set_keyboard_color(cc, zone,
							  cc->keyboard_color[zone]);
	}
	if (err)
		return err;
	if (word) {
		err = clevo_cc_keyboard_command(cc, word);
		if (err)
			return err;
	}
	err = clevo_cc_disable_keyboard_sleep_timer(cc);
	if (err)
		return err;

	cc->keyboard_mode = name;
	cc->keyboard_led.brightness = cc->keyboard_brightness_raw;
	return 0;
}

/*
 * sysfs: keyboard_rgb
 *
 * This is deliberately a small, named interface instead of exposing arbitrary
 * command-103 words. The accepted forms are:
 *
 *   all 112233
 *   mode off | static | random | breath | cycle | wave | dance | tempo | flash
 *   brightness 0..100
 *   raw-brightness 0..255
 *   probe 0..2 112233
 *
 * `mode` selects the firmware's own RGB15 effect (the words come from the
 * vendor's RGBKB.SetMode for kb_type 6/22); `static` re-applies the persisted
 * colors and `off` disables the channel. Colors are written as RRGGBB.
 * `brightness` is a percentage: the RGB15 channel is analog, so it is scaled
 * onto the raw 0..191 byte (100% = EC maximum) rather than quantised onto the
 * vendor's five calibrated steps. `raw-brightness` stays available as an
 * experimental, uncached byte-level diagnostic.
 *
 * This P15 23 has one physical RGB15 channel; the legacy left/middle/right
 * spellings remain accepted as aliases for compatibility, but all of them
 * address the same F0 channel. `probe` is an experimental, uncached F0/F1/F2
 * diagnostic. `raw-brightness` and `probe` should only be used with the daemon
 * stopped.
 */
static ssize_t keyboard_rgb_store(struct device *dev,
					 struct device_attribute *attr,
					 const char *buf, size_t count)
{
	struct clevo_cc *cc = dev_get_drvdata(dev);
	char *input, *copy, *op, *value, *extra, *probe_zone = NULL;
	u8 color[3];
	int err = -EINVAL;

	input = kstrdup(buf, GFP_KERNEL);
	if (!input)
		return -ENOMEM;
	copy = input;
	op = strsep(&copy, " \t\n");
	while (op && *op == '\0')
		op = strsep(&copy, " \t\n");
	value = strsep(&copy, " \t\n");
	while (value && *value == '\0')
		value = strsep(&copy, " \t\n");
	if (!op || !value)
		goto out;
	if (!strcmp(op, "probe")) {
		probe_zone = value;
		value = strsep(&copy, " \t\n");
		while (value && *value == '\0')
			value = strsep(&copy, " \t\n");
		if (!value)
			goto out;
	}
	extra = strsep(&copy, " \t\n");
	while (extra && *extra == '\0')
		extra = strsep(&copy, " \t\n");
	if (extra)
		goto out;

	mutex_lock(&cc->keyboard_lock);
	if (!strcmp(op, "mode")) {
		unsigned int i;

		if (!strcmp(value, "off")) {
			err = clevo_cc_set_keyboard_brightness_percent(cc, 0);
			if (!err)
				err = clevo_cc_set_keyboard_status(cc, false);
			if (!err)
				err = clevo_cc_disable_keyboard_sleep_timer(cc);
			if (!err) {
				cc->keyboard_mode = "off";
				cc->keyboard_led.brightness = LED_OFF;
			}
		} else if (!strcmp(value, "static")) {
			err = clevo_cc_apply_keyboard_effect(cc, "static", 0);
		} else {
			err = -EINVAL;
			for (i = 0; i < ARRAY_SIZE(clevo_cc_keyboard_effects); i++) {
				if (strcmp(value,
					   clevo_cc_keyboard_effects[i].name))
					continue;
				err = clevo_cc_apply_keyboard_effect(
					cc, clevo_cc_keyboard_effects[i].name,
					clevo_cc_keyboard_effects[i].word);
				break;
			}
		}
	} else if (!strcmp(op, "brightness")) {
		unsigned int percent;

		/*
		 * Named brightness is a percentage (0..100). `raw-brightness`
		 * below remains the uncached 0..191 byte for diagnostics.
		 */
		if (!kstrtouint(value, 10, &percent) &&
		    percent <= CLEVO_KB_BRIGHTNESS_PERCENT_MAX)
			err = clevo_cc_set_keyboard_brightness_percent(cc,
								       percent);
		if (!err && percent == 0)
			err = clevo_cc_set_keyboard_status(cc, false);
		if (!err && percent != 0)
			err = clevo_cc_disable_keyboard_sleep_timer(cc);
		if (!err)
			cc->keyboard_brightness = percent;
		if (!err) {
			cc->keyboard_brightness_raw =
				clevo_cc_keyboard_percent_raw(percent);
			cc->keyboard_led.brightness =
				cc->keyboard_brightness_raw;
		}
	} else if (!strcmp(op, "raw-brightness")) {
		unsigned int raw;

		if (kstrtouint(value, 10, &raw) || raw > 255)
			goto unlock;
		err = clevo_cc_set_keyboard_status(cc, true);
		if (!err)
			err = clevo_cc_set_keyboard_brightness_raw(cc, raw);
		if (!err)
			err = clevo_cc_disable_keyboard_sleep_timer(cc);
	} else if (!strcmp(op, "all") || !strcmp(op, "left") ||
		   !strcmp(op, "middle") || !strcmp(op, "right")) {
		int zone;

		err = clevo_cc_parse_keyboard_color(value, color);
		if (err)
			goto unlock;
		err = clevo_cc_transition_keyboard_color(cc, color);
		if (!err) {
			err = clevo_cc_disable_keyboard_sleep_timer(cc);
		}
		if (!err) {
			for (zone = 0; zone < 3; zone++) {
				memcpy(cc->keyboard_color[zone], color, sizeof(color));
				cc->keyboard_color_known[zone] = true;
			}
		}
	} else if (!strcmp(op, "probe")) {
		unsigned int zone;

		if (!probe_zone || kstrtouint(probe_zone, 10, &zone) || zone >= 3)
			goto unlock;
		err = clevo_cc_parse_keyboard_color(value, color);
		if (err)
			goto unlock;
		err = clevo_cc_probe_keyboard_color(cc, zone, color);
		if (!err)
			err = clevo_cc_disable_keyboard_sleep_timer(cc);
	}
unlock:
	mutex_unlock(&cc->keyboard_lock);
out:
	kfree(input);
	return err ? err : count;
}
static DEVICE_ATTR_RW(keyboard_rgb);

static const char *clevo_cc_perf_name(enum clevo_perf_mode mode)
{
	switch (mode) {
	case CLEVO_PERF_PWRSAVING:
		return "pwrsaving";
	case CLEVO_PERF_PERFORMANCE:
		return "performance";
	case CLEVO_PERF_ENTERTAINMENT:
		return "entertainment";
	default:
		return "quiet";
	}
}

/*
 * Read fan speeds (command 12).
 *
 * The reply carries a rotation period for each fan; rpm is derived with the
 * Control Center formula `2156250 / period`.
 *
 * Temperature is *not* returned here: the CPU byte at [18] needs the vendor's
 * `CalCPUTemp` curve (a TDP-class-dependent piecewise function), which belongs
 * in userspace where the CPU model is known, and the GPU bytes at [21]/[24] are
 * direct Celsius. hwmon callers that want a temperature use
 * clevo_cc_read_temp(), which reads those directly.
 */
static int clevo_cc_read_fan(struct clevo_cc *cc, u32 *cpu_rpm, u32 *gpu_rpm)
{
	u8 payload[CLEVO_PAYLOAD_LEN];
	size_t len = 0;
	u16 cpu_period, gpu_period;
	int err;

	err = clevo_cc_dsm_buffer(cc, CLEVO_CMD_FAN_STATUS, payload,
				  sizeof(payload), &len);
	if (err)
		return err;

	if (len < CLEVO_FAN_STATUS_MIN_LEN)
		return -EPROTO;

	/* Command 12 stores the rotation period, big-endian. */
	cpu_period = (payload[2] << 8) | payload[3];
	gpu_period = (payload[4] << 8) | payload[5];

	/* rpm = 60 / (5.565217391304348e-05 * period) * 2 = 2156250 / period */
	*cpu_rpm = cpu_period ? DIV_ROUND_CLOSEST(2156250, cpu_period) : 0;
	*gpu_rpm = gpu_period ? DIV_ROUND_CLOSEST(2156250, gpu_period) : 0;
	return 0;
}

/*
 * Read a GPU temperature (command 12, offsets [21] and [24]).
 *
 * These are direct degrees Celsius. A 0 means the EC reports nothing, which
 * becomes -ENODATA so hwmon shows the channel as absent rather than 0 °C.
 *
 * The CPU temperature is deliberately not exposed by this driver: its raw byte
 * at [18] requires the `CalCPUTemp` TDP-class curve, and the kernel has no
 * reliable way to learn the CPU's TDP class. Userspace (`clevod`) applies it.
 */
static int clevo_cc_read_gpu_temp(struct clevo_cc *cc, int channel, long *val)
{
	u8 payload[CLEVO_PAYLOAD_LEN];
	size_t len = 0;
	u8 offset;
	int err;

	if (channel == 0)
		return -EOPNOTSUPP; /* CPU: needs a userspace conversion */
	if (channel != 1)
		return -EOPNOTSUPP;

	offset = 21; /* GPU1; GPU2 lives at [24] and is absent on most machines */

	err = clevo_cc_dsm_buffer(cc, CLEVO_CMD_FAN_STATUS, payload,
				  sizeof(payload), &len);
	if (err)
		return err;
	if (len < CLEVO_FAN_STATUS_MIN_LEN)
		return -EPROTO;

	if (payload[offset] == 0)
		return -ENODATA;

	*val = (long)payload[offset] * 1000; /* hwmon wants millidegrees */
	return 0;
}

/*
 * Read the fan curve (command 13) into `out` (256 bytes).
 *
 * This is the read side of the future custom-curve interface; the writable
 * curve path (command 14) will be added together with the graphical editor.
 */
static int clevo_cc_read_curve(struct clevo_cc *cc, u8 *out, size_t out_cap,
			       size_t *out_len)
{
	return clevo_cc_dsm_buffer(cc, CLEVO_CMD_FAN_CURVE, out, out_cap,
				   out_len);
}

static umode_t clevo_cc_is_visible(const void *data, enum hwmon_sensor_types type,
				   u32 attr, int channel)
{
	switch (type) {
	case hwmon_fan:
		return attr == hwmon_fan_input ? 0444 : 0;
	case hwmon_temp:
		return attr == hwmon_temp_input ? 0444 : 0;
	default:
		return 0;
	}
}

/*
 * hwmon temperature inputs. Channel 1 is the GPU.
 *
 * The CPU channel is intentionally not registered: its raw byte needs the
 * vendor's TDP-class conversion, which belongs in userspace. Userspace reads
 * `raw_status` (or command 12 directly) and applies `CalCPUTemp`.
 */
static int clevo_cc_read_temp(struct device *dev, u32 attr, int channel,
			      long *val)
{
	struct clevo_cc *cc = dev_get_drvdata(dev);

	if (attr != hwmon_temp_input)
		return -EOPNOTSUPP;

	return clevo_cc_read_gpu_temp(cc, channel, val);
}

static int clevo_cc_read(struct device *dev, enum hwmon_sensor_types type,
			 u32 attr, int channel, long *val)
{
	struct clevo_cc *cc = dev_get_drvdata(dev);
	u32 cpu_rpm, gpu_rpm;
	int err;

	if (type == hwmon_temp)
		return clevo_cc_read_temp(dev, attr, channel, val);

	if (type != hwmon_fan || attr != hwmon_fan_input)
		return -EOPNOTSUPP;

	err = clevo_cc_read_fan(cc, &cpu_rpm, &gpu_rpm);
	if (err)
		return err;

	switch (channel) {
	case 0:
		*val = cpu_rpm;
		break;
	case 1:
		*val = gpu_rpm;
		break;
	default:
		return -EOPNOTSUPP;
	}
	return 0;
}

static const struct hwmon_channel_info *clevo_cc_info[] = {
	HWMON_CHANNEL_INFO(fan, HWMON_F_INPUT, HWMON_F_INPUT),
	HWMON_CHANNEL_INFO(temp, 0, HWMON_T_INPUT),
	NULL
};

static const struct hwmon_ops clevo_cc_hwmon_ops = {
	.is_visible = clevo_cc_is_visible,
	.read = clevo_cc_read,
};

static const struct hwmon_chip_info clevo_cc_chip_info = {
	.ops = &clevo_cc_hwmon_ops,
	.info = clevo_cc_info,
};

/*
 * sysfs: fan_mode
 *
 * Read returns the last mode written this session; write accepts
 * "auto", "quiet" or "max". The firmware does not report the current mode
 * reliably, so the driver only reflects what it has set.
 *
 *  - auto:  command 121/1 value 0, and clear any maximum offset
 *  - quiet: command 121/1 value 8, and clear any maximum offset
 *  - max:   command 121/14 value 255 (maximum fan offset, +100%)
 */
static const char *clevo_cc_mode_name(enum clevo_fan_mode mode)
{
	switch (mode) {
	case CLEVO_MODE_QUIET:
		return "quiet";
	case CLEVO_MODE_MAX:
		return "max";
	case CLEVO_MODE_MAXQ:
		return "maxq";
	case CLEVO_MODE_CUSTOM:
		return "custom";
	default:
		return "auto";
	}
}

static ssize_t fan_mode_show(struct device *dev, struct device_attribute *attr,
			     char *buf)
{
	struct clevo_cc *cc = dev_get_drvdata(dev);

	return sysfs_emit(buf, "%s\n", clevo_cc_mode_name(cc->fan_mode));
}

static ssize_t fan_mode_store(struct device *dev, struct device_attribute *attr,
			      const char *buf, size_t count)
{
	struct clevo_cc *cc = dev_get_drvdata(dev);
	enum clevo_fan_mode mode;
	u32 value;
	int err;

	if (sysfs_streq(buf, "auto")) {
		mode = CLEVO_MODE_AUTO;
		value = CLEVO_FAN_MODE_AUTO;
	} else if (sysfs_streq(buf, "quiet")) {
		mode = CLEVO_MODE_QUIET;
		value = CLEVO_FAN_MODE_QUIET;
	} else if (sysfs_streq(buf, "max")) {
		mode = CLEVO_MODE_MAX;
		value = CLEVO_FAN_MODE_MAXIMUM;
	} else if (sysfs_streq(buf, "maxq")) {
		mode = CLEVO_MODE_MAXQ;
		value = CLEVO_FAN_MODE_MAXQ;
	} else if (sysfs_streq(buf, "custom")) {
		/*
		 * Selecting "custom" only makes the firmware use the curve
		 * stored in the EC; write one through `fan_curve` first.
		 */
		mode = CLEVO_MODE_CUSTOM;
		value = CLEVO_FAN_MODE_CUSTOM;
	} else {
		return -EINVAL;
	}

	err = clevo_cc_set_fan_mode(cc, value);
	if (err)
		return err;

	cc->fan_mode = mode;
	return count;
}
static DEVICE_ATTR_RW(fan_mode);

/*
 * sysfs: fan_curve
 *
 * Reports the current curve and accepts a new one.
 *
 * Read format: "fan_count=<n> kb_type=<k> cpu:T1,D1,... gpu1:... gpu2:..." with
 * D as raw 0..255.
 *
 * Write format: the same per-fan point lists, e.g.
 *
 *   echo "cpu: 0,0 45,76 70,204 0,0" > fan_curve
 *
 * Only points 2 and 3 are sent to the EC (command 14), matching the Windows
 * stack: T1/D1 and T4/D4 are not part of the write payload. A fan list whose
 * middle two points are zero is skipped, so a two-fan machine never has to
 * invent points for a fan it does not have.
 *
 * The write is a **read-modify-write**: the current curve is fetched first and
 * the named channels are merged into it. This matters because command 14
 * replaces the whole table - sending a payload with zeros for a channel would
 * wipe that channel's curve. A caller who only wants to change the CPU curve
 * therefore does not have to resend the GPU one.
 *
 * The read and write payloads do **not** share a layout, so the merge goes
 * through the decoded curve, never through a raw buffer copy:
 *
 *   command 13 (read):  [2..3] = CPU *fan period*, curves at [0x10..0x27]
 *   command 14 (write): [2..3] = F1T2/F1D2,      curves at [2..0x0d]
 *
 * Copying the read buffer into the write payload would feed fan periods in as
 * curve points.
 *
 * Writing does *not* select "custom"; echo custom > fan_mode does that.
 */
static ssize_t fan_curve_show(struct device *dev, struct device_attribute *attr,
			      char *buf)
{
	struct clevo_cc *cc = dev_get_drvdata(dev);
	u8 payload[CLEVO_PAYLOAD_LEN];
	size_t len = 0;
	int err, i, n = 0;

	err = clevo_cc_read_curve(cc, payload, sizeof(payload), &len);
	if (err)
		return err;
	if (len < 0x28)
		return -EPROTO;

	n += sysfs_emit_at(buf, n, "fan_count=%u kb_type=%u\n", payload[0x0c],
			   payload[0x0f]);
	for (i = 0; i < 3 && n < PAGE_SIZE - 64; i++) {
		const char *name = i == 0 ? "cpu" : (i == 1 ? "gpu1" : "gpu2");
		int o = 0x10 + i * 8;

		n += sysfs_emit_at(buf, n, "%s: %u,%u %u,%u %u,%u %u,%u\n", name,
				   payload[o], payload[o + 1], payload[o + 2],
				   payload[o + 3], payload[o + 4], payload[o + 5],
				   payload[o + 6], payload[o + 7]);
	}
	return n;
}

/*
 * Parse one fan's point list.
 *
 * The EC's write path (command 14) only carries points 2 and 3; T1/D1 and T4/D4
 * are not part of the payload (the EC keeps its own first and last point). The
 * read side still emits all four for symmetry with `fan_curve_show`, so the
 * parser accepts four points but only *uses* the middle two. Points sent as
 * zero are ignored, which is why a caller can write
 * `cpu: 0,0 45,76 70,204 0,0` without inventing an endpoint.
 *
 * Returns the number of points read, or -EINVAL on a malformed list.
 */
static int clevo_cc_parse_points(const char *text, u8 temps[4], u8 duties[4])
{
	const char *p = text;
	int count = 0;

	while (count < 4) {
		unsigned int t, d;
		int consumed = 0;

		/* Skip separators. */
		while (*p == ' ' || *p == '\t' || *p == ',')
			p++;
		if (*p == '\0' || *p == '\n')
			break;
		if (sscanf(p, "%u,%u%n", &t, &d, &consumed) != 2)
			return -EINVAL;
		if (t > 255 || d > 255)
			return -EINVAL;
		temps[count] = (u8)t;
		duties[count] = (u8)d;
		count++;
		p += consumed;
	}
	return count;
}

/*
 * Encode one fan's two writable points (T2/D2, T3/D3) into `payload`.
 *
 * `slope_base` is the payload offset of that fan's first slope word. Only R2
 * (the T2->T3 segment, the one the write fully specifies) is computed; R1 and
 * R3 depend on T1/T4, which are not sent and are owned by the EC.
 *
 * Returns -EINVAL when the channel cannot be encoded (non-increasing
 * temperatures). The caller decides whether that is fatal.
 */
static int clevo_cc_encode_fan(u8 *payload, int base, int slope_base,
			       const u8 temps[4], const u8 duties[4])
{
	if (temps[2] <= temps[1])
		return -EINVAL;

	payload[base] = temps[1];
	payload[base + 1] = duties[1];
	payload[base + 2] = temps[2];
	payload[base + 3] = duties[2];

	/*
	 * R2 = round((raw(D3) - raw(D2)) / (T3 - T2) * 16), big-endian, at the
	 * fan's second slope slot. The duty bytes are already raw 0..255.
	 */
	{
		int dt = (int)temps[2] - (int)temps[1];
		int dd = (int)duties[2] - (int)duties[1];
		long slope = DIV_ROUND_CLOSEST((long)dd * 16, dt);
		int off = slope_base + 2;

		slope = clamp_t(long, slope, 0, 0xFFFF);
		payload[off] = (u8)(slope >> 8);
		payload[off + 1] = (u8)(slope & 0xFF);
	}
	return 0;
}

/*
 * Recompute every fan's R2 from the points already in `payload`.
 *
 * Used after seeding from a read: the read reply carries no write-form slopes,
 * so they are derived from the points that were just copied. A channel whose
 * T2/T3 are zero (absent) is skipped.
 */
static void clevo_cc_fill_slopes(u8 *payload)
{
	static const int base[3] = { 2, 6, 10 };
	static const int slope_base[3] = { 14, 20, 26 };
	int fan;

	for (fan = 0; fan < 3; fan++) {
		u8 t2 = payload[base[fan]];
		u8 d2 = payload[base[fan] + 1];
		u8 t3 = payload[base[fan] + 2];
		u8 d3 = payload[base[fan] + 3];
		int dt, dd, off;
		long slope;

		if (!t2 && !t3)
			continue; /* absent channel */
		dt = (int)t3 - (int)t2;
		if (dt <= 0)
			continue; /* leave the slopes zero */

		dd = (int)d3 - (int)d2;
		slope = clamp_t(long, DIV_ROUND_CLOSEST((long)dd * 16, dt), 0,
				0xFFFF);
		off = slope_base[fan] + 2;
		payload[off] = (u8)(slope >> 8);
		payload[off + 1] = (u8)(slope & 0xFF);
	}
}

/*
 * Convert a command-13 curve reply into a command-14 write payload.
 *
 * The two commands use different layouts, so the points are decoded by offset
 * and re-emitted in the write form. Only the two points the write carries are
 * copied; the write's slope words are left for the caller to fill (or zero,
 * which the firmware recomputes).
 *
 *   read  (cmd 13): cpu at [0x10], gpu1 at [0x18], gpu2 at [0x20]; each point
 *                   is (T, D) with D raw 0..255
 *   write (cmd 14): cpu T2/D2/T3/D3 at [2..5], gpu1 at [6..9], gpu2 at [10..13]
 */
static void clevo_cc_curve_to_write_payload(const u8 *read_reply,
					    u8 *payload)
{
	static const int read_off[3] = { 0x10, 0x18, 0x20 };
	static const int write_off[3] = { 2, 6, 10 };
	int fan, i;

	for (fan = 0; fan < 3; fan++) {
		const u8 *src = read_reply + read_off[fan];
		u8 *dst = payload + write_off[fan];

		/* Points 2 and 3 (index 1 and 2). */
		for (i = 0; i < 2; i++) {
			dst[i * 2] = src[(i + 1) * 2];     /* T(n+1) */
			dst[i * 2 + 1] = src[(i + 1) * 2 + 1]; /* D(n+1) */
		}
	}
}

/*
 * sysfs helper: read the EC curve and seed `payload` with it in write form.
 *
 * Returns 0 on success, or -errno. On failure `payload` must not be sent.
 */
static int clevo_cc_seed_write_payload(struct clevo_cc *cc, u8 *payload)
{
	u8 reply[CLEVO_PAYLOAD_LEN];
	size_t len = 0;
	int err;

	err = clevo_cc_read_curve(cc, reply, sizeof(reply), &len);
	if (err)
		return err;
	if (len < 0x28)
		return -EPROTO;

	memset(payload, 0, CLEVO_PAYLOAD_LEN);
	clevo_cc_curve_to_write_payload(reply, payload);
	clevo_cc_fill_slopes(payload);
	return 0;
}

static ssize_t fan_curve_store(struct device *dev, struct device_attribute *attr,
			       const char *buf, size_t count)
{
	struct clevo_cc *cc = dev_get_drvdata(dev);
	u8 payload[CLEVO_PAYLOAD_LEN] = { 0 };
	char *copy, *line;
	int err;

	/*
	 * Seed from the EC's current table, translated into the write layout:
	 * command 14 replaces all of it, so a payload without a channel's
	 * points would wipe that channel. If the read fails there is nothing
	 * safe to merge into, so refuse rather than send a mostly-zero table.
	 */
	err = clevo_cc_seed_write_payload(cc, payload);
	if (err)
		return err;

	copy = kstrdup(buf, GFP_KERNEL);
	if (!copy)
		return -ENOMEM;
	err = 0;

	for (line = strsep(&copy, "\n"); line; line = strsep(&copy, "\n")) {
		u8 temps[4] = { 0 }, duties[4] = { 0 };
		const char *sep;
		int base, slope_base, n;

		/* Strip leading whitespace. */
		while (*line == ' ' || *line == '\t')
			line++;
		if (*line == '\0')
			continue;

		sep = strchr(line, ':');
		if (!sep) {
			/*
			 * `fan_count=` / `kb_type=` are informational lines from the
			 * read side. They use '=' rather than ':', so recognise them
			 * before demanding a separator - otherwise echoing a read
			 * back (which is how a curve is restored) fails with EINVAL.
			 */
			if (!strncmp(line, "fan_count", 9) ||
			    !strncmp(line, "kb_type", 7))
				continue;
			err = -EINVAL;
			break;
		}
		if (!strncmp(line, "cpu", 3)) {
			base = 2; /* payload offset of F1T2 */
			slope_base = 14;
		} else if (!strncmp(line, "gpu1", 4)) {
			base = 6;
			slope_base = 20;
		} else if (!strncmp(line, "gpu2", 4)) {
			base = 10;
			slope_base = 26;
		} else if (!strncmp(line, "fan_count", 9) ||
			 !strncmp(line, "kb_type", 7)) {
			continue; /* informational lines from the read side */
		} else {
			err = -EINVAL;
			break;
		}

		n = clevo_cc_parse_points(sep + 1, temps, duties);
		if (n != 4) {
			dev_warn(&cc->adev->dev,
				 "fan_curve: line %.16s: parsed %d points, need 4\n",
				 line, n);
			err = -EINVAL;
			break;
		}

		/*
		 * A channel with no middle points is "leave this one alone":
		 * its values stay as seeded above.
		 */
		if (!temps[1] && !temps[2] && !duties[1] && !duties[2])
			continue;

		/*
		 * The user named this channel, so bad values are an error - but
		 * only if the values actually changed. A corrupt channel read
		 * back from the EC and echoed unchanged (which is what a
		 * restore does) must not block the write, or a bad table could
		 * never be repaired.
		 */
		if (!clevo_cc_encode_fan(payload, base, slope_base, temps,
					 duties)) {
			/* Encoded fine. */
			continue;
		}

		if (temps[1] != payload[base] || temps[2] != payload[base + 2] ||
		    duties[1] != payload[base + 1] ||
		    duties[2] != payload[base + 3]) {
			dev_warn(&cc->adev->dev,
				 "fan_curve: line %.16s: T2=%u T3=%u cannot encode; "
				 "EC has T2=%u D2=%u T3=%u D3=%u\n",
				 line, temps[1], temps[2], payload[base],
				 payload[base + 1], payload[base + 2],
				 payload[base + 3]);
			err = -EINVAL;
			break;
		}
		/* Unchanged and unusable: leave the channel as the EC had it. */
	}
	kfree(copy);
	if (err)
		return err;

	{
		u8 reply[CLEVO_PAYLOAD_LEN];
		size_t reply_len = 0;

		err = clevo_cc_dsm_payload(cc, CLEVO_CMD_FAN_CURVE_WRITE,
					   payload, reply, sizeof(reply),
					   &reply_len);
	}
	if (err)
		return err;

	return count;
}
static DEVICE_ATTR_RW(fan_curve);

/*
 * sysfs: raw_status (diagnostic)
 *
 * Dumps the raw command-12 reply as hex so the duty/temperature offsets can be
 * verified against the EC's actual bytes instead of inferred. Read-only.
 */
static ssize_t raw_status_show(struct device *dev, struct device_attribute *attr,
			       char *buf)
{
	struct clevo_cc *cc = dev_get_drvdata(dev);
	u8 payload[CLEVO_PAYLOAD_LEN];
	size_t len = 0;
	int err, i, n = 0;

	err = clevo_cc_dsm_buffer(cc, CLEVO_CMD_FAN_STATUS, payload,
				  sizeof(payload), &len);
	if (err)
		return err;

	n += sysfs_emit_at(buf, n, "len=%zu\n", len);
	for (i = 0; i < (int)len && n < PAGE_SIZE - 8; i++)
		n += sysfs_emit_at(buf, n, "%02x", payload[i]);
	n += sysfs_emit_at(buf, n, "\n");
	return n;
}
static DEVICE_ATTR_RO(raw_status);

/*
 * sysfs: raw_curve (diagnostic)
 *
 * Dumps the raw command-13 reply as hex, for the same reason as raw_status.
 */
static ssize_t raw_curve_show(struct device *dev, struct device_attribute *attr,
			      char *buf)
{
	struct clevo_cc *cc = dev_get_drvdata(dev);
	u8 payload[CLEVO_PAYLOAD_LEN];
	size_t len = 0;
	int err, i, n = 0;

	err = clevo_cc_read_curve(cc, payload, sizeof(payload), &len);
	if (err)
		return err;

	n += sysfs_emit_at(buf, n, "len=%zu\n", len);
	for (i = 0; i < (int)len && n < PAGE_SIZE - 8; i++)
		n += sysfs_emit_at(buf, n, "%02x", payload[i]);
	n += sysfs_emit_at(buf, n, "\n");
	return n;
}
static DEVICE_ATTR_RO(raw_curve);

/*
 * sysfs: perf_mode
 *
 * Read reports the last value written this session (or "unknown" if nothing
 * has been set, since the firmware does not report the current mode reliably).
 * Write accepts quiet / pwrsaving / performance / entertainment, mapped to
 * DCHU 121/25 values 0..3.
 */
static ssize_t perf_mode_show(struct device *dev, struct device_attribute *attr,
			      char *buf)
{
	struct clevo_cc *cc = dev_get_drvdata(dev);

	if (!cc->perf_mode_set)
		return sysfs_emit(buf, "unknown\n");
	return sysfs_emit(buf, "%s\n", clevo_cc_perf_name(cc->perf_mode));
}

static ssize_t perf_mode_store(struct device *dev, struct device_attribute *attr,
			       const char *buf, size_t count)
{
	struct clevo_cc *cc = dev_get_drvdata(dev);
	enum clevo_perf_mode mode;
	int err;

	if (sysfs_streq(buf, "quiet"))
		mode = CLEVO_PERF_QUIET;
	else if (sysfs_streq(buf, "pwrsaving"))
		mode = CLEVO_PERF_PWRSAVING;
	else if (sysfs_streq(buf, "performance"))
		mode = CLEVO_PERF_PERFORMANCE;
	else if (sysfs_streq(buf, "entertainment"))
		mode = CLEVO_PERF_ENTERTAINMENT;
	else
		return -EINVAL;

	err = clevo_cc_set_perf_mode(cc, mode);
	if (err)
		return err;

	cc->perf_mode = mode;
	cc->perf_mode_set = true;
	return count;
}
static DEVICE_ATTR_RW(perf_mode);

static struct attribute *clevo_cc_attrs[] = {
	&dev_attr_fan_mode.attr,
	&dev_attr_fan_curve.attr,
	&dev_attr_perf_mode.attr,
	&dev_attr_raw_status.attr,
	&dev_attr_raw_curve.attr,
	&dev_attr_keyboard_rgb.attr,
	NULL
};
ATTRIBUTE_GROUPS(clevo_cc);

static int clevo_cc_probe(struct platform_device *pdev)
{
	struct acpi_device *adev = ACPI_COMPANION(&pdev->dev);
	struct clevo_cc *cc;
	struct device *hwmon;
	acpi_handle h;
	acpi_status status;

	if (!adev)
		return -ENODEV;

	cc = devm_kzalloc(&pdev->dev, sizeof(*cc), GFP_KERNEL);
	if (!cc)
		return -ENOMEM;
	cc->adev = adev;
	mutex_init(&cc->keyboard_lock);
	cc->keyboard_brightness = 100;
	cc->keyboard_brightness_raw = CLEVO_KB_BRIGHTNESS_MAX;
	cc->keyboard_mode = "unknown";
	cc->keyboard_led.name = "clevo::kbd_backlight";
	cc->keyboard_led.max_brightness = CLEVO_KB_BRIGHTNESS_MAX;
	cc->keyboard_led.brightness = CLEVO_KB_BRIGHTNESS_MAX;
	cc->keyboard_led.flags = LED_CORE_SUSPENDRESUME;
	cc->keyboard_led.brightness_set_blocking = clevo_cc_keyboard_led_set;
	cc->keyboard_led.brightness_get = clevo_cc_keyboard_led_get;
	status = devm_led_classdev_register(&pdev->dev, &cc->keyboard_led);
	if (status) {
		dev_err(&pdev->dev, "failed to register keyboard backlight LED: %d\n",
			status);
		return status;
	}

	status = acpi_get_handle(adev->handle, "_DSM", &h);
	if (ACPI_FAILURE(status)) {
		dev_err(&pdev->dev, "firmware has no _DSM method\n");
		return -ENODEV;
	}

	/* Copy the verified GUID bytes verbatim. */
	memcpy(cc->dsm_guid,
	       "\xe4\x24\xf2\x93\xdc\xfb\xbf\x4b\xad\xd6\xdb\x71\xbd\xc0\xaf\xad",
	       16);

	platform_set_drvdata(pdev, cc);

	hwmon = devm_hwmon_device_register_with_info(
		&pdev->dev, "clevo_cc", cc, &clevo_cc_chip_info, NULL);
	if (IS_ERR(hwmon))
		return PTR_ERR(hwmon);

	dev_info(&pdev->dev, "clevo-cc fan and RGB15 control registered\n");
	return 0;
}

static const struct acpi_device_id clevo_cc_acpi_ids[] = {
	{ "CLV0001", 0 },
	{ }
};
MODULE_DEVICE_TABLE(acpi, clevo_cc_acpi_ids);

static struct platform_driver clevo_cc_driver = {
	.probe = clevo_cc_probe,
	.driver = {
		.name = "clevo-cc",
		.acpi_match_table = clevo_cc_acpi_ids,
		.dev_groups = clevo_cc_groups,
	},
};
module_platform_driver(clevo_cc_driver);

MODULE_AUTHOR("clevo-cc-linux");
MODULE_DESCRIPTION("Clevo DCHU fan and RGB15 control (ACPI _DSM)");
MODULE_LICENSE("GPL");
