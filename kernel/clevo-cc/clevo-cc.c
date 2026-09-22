// SPDX-License-Identifier: GPL-2.0-only
/*
 * clevo-cc - read-only ACPI platform driver for Clevo DCHU devices.
 *
 * This is slice S6a: it binds ACPI\CLV0001, evaluates the firmware `_DSM`
 * method using a real ACPI Package argument (which acpi_call cannot build),
 * and exposes the verified read commands through hwmon:
 *
 *   fan1_input  - CPU fan speed in rpm
 *   fan2_input  - GPU1 fan speed in rpm
 *
 * Only read commands are issued. The 256-byte payload is sent as Arg3 =
 * Package { Buffer(256) } as required by the DSDT.
 *
 * Verified against a COLORFUL P15 23; see docs/hardware-notes.md.
 */

#include <linux/acpi.h>
#include <linux/hwmon.h>
#include <linux/hwmon-sysfs.h>
#include <linux/module.h>
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

/* CLEVO_CMD_MAIN sub-command for fan mode. */
#define CLEVO_SUB_FAN_MODE 1

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
};

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
		/*
		 * The SCMD/GCMD families return the command number itself on
		 * success (e.g. 0x79 for 121); 0x80000002 means "not
		 * supported" and anything else is unexpected. Treat a return
		 * equal to the function as success.
		 */
		if (ret->integer.value == function) {
			err = 0;
		} else if (ret->integer.value == 0x80000002) {
			dev_warn(&cc->adev->dev,
				 "_DSM function %u returned 0x80000002 (unsupported)\n",
				 function);
			err = -EOPNOTSUPP;
		} else {
			dev_warn(&cc->adev->dev,
				 "_DSM function %u returned unexpected integer 0x%llx\n",
				 function, ret->integer.value);
			err = -EIO;
		}
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

	pkg.type = ACPI_TYPE_PACKAGE;
	pkg.package.count = 1;
	pkg.package.elements = kcalloc(1, sizeof(union acpi_object), GFP_KERNEL);
	if (!pkg.package.elements)
		return -ENOMEM;
	pkg.package.elements[0].type = ACPI_TYPE_INTEGER;
	pkg.package.elements[0].integer.value = value;

	return clevo_cc_dsm_call(cc, function, &pkg, out, out_cap, out_len);
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
 *   echo "cpu: 0,0 55,102 75,178 0,0" > fan_curve
 *
 * Only points 2 and 3 are sent to the EC (command 14), matching the Windows
 * stack: T1/D1 and T4/D4 are not part of the write payload. A fan list whose
 * temperatures are all zero is skipped, so a two-fan machine never has to
 * invent points for a fan it does not have. Writing does *not* select "custom";
 * echo custom > fan_mode does that.
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
 * Parse one fan's point list ("T,D T,D T,D T,D") into `out`, returning the
 * number of points read. Accepts the four-point form the read side emits; only
 * points 2 and 3 are used by the write payload.
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

static ssize_t fan_curve_store(struct device *dev, struct device_attribute *attr,
			       const char *buf, size_t count)
{
	struct clevo_cc *cc = dev_get_drvdata(dev);
	u8 payload[CLEVO_PAYLOAD_LEN] = { 0 };
	char *copy, *line;
	int err = 0;

	copy = kstrdup(buf, GFP_KERNEL);
	if (!copy)
		return -ENOMEM;

	for (line = strsep(&copy, "\n"); line; line = strsep(&copy, "\n")) {
		u8 temps[4] = { 0 }, duties[4] = { 0 };
		const char *sep;
		int base, slope_base, n, i;

		/* Strip leading whitespace. */
		while (*line == ' ' || *line == '\t')
			line++;
		if (*line == '\0')
			continue;

		sep = strchr(line, ':');
		if (!sep) {
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
			err = -EINVAL;
			break;
		}

		/* A wholly zero fan means "leave this channel alone". */
		if (!temps[0] && !temps[1] && !temps[2] && !temps[3] &&
		    !duties[0] && !duties[1] && !duties[2] && !duties[3])
			continue;
		if (temps[2] <= temps[1]) {
			err = -EINVAL;
			break;
		}

		payload[base] = temps[1];
		payload[base + 1] = duties[1];
		payload[base + 2] = temps[2];
		payload[base + 3] = duties[2];

		/*
		 * Slopes in raw-duty units scaled by 16, big-endian:
		 *   round((raw(D(n+1)) - raw(Dn)) / (T(n+1) - Tn) * 16)
		 * with raw(p) = p * 255 / 100. Computed with the *sent* duty
		 * bytes so the firmware and the driver agree on the value.
		 */
		for (i = 0; i < 3; i++) {
			int dt = (int)temps[i + 1] - (int)temps[i];
			int dd = (int)duties[i + 1] - (int)duties[i];
			long slope;
			int off = slope_base + i * 2;

			if (dt <= 0) {
				err = -EINVAL;
				goto out;
			}
			slope = DIV_ROUND_CLOSEST((long)dd * 16, dt);
			slope = clamp_t(long, slope, 0, 0xFFFF);
			payload[off] = (u8)(slope >> 8);
			payload[off + 1] = (u8)(slope & 0xFF);
		}
	}
	/* Fall through to `out` with the parse error, if any. */
out:
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

	dev_info(&pdev->dev, "clevo-cc read-only fan monitoring registered\n");
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
MODULE_DESCRIPTION("Clevo DCHU fan monitoring and fan-mode control (ACPI _DSM)");
MODULE_LICENSE("GPL");
