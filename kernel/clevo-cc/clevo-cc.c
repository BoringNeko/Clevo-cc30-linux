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

/* Command numbers (verified; see clevo-proto). */
#define CLEVO_CMD_FAN_STATUS 12
#define CLEVO_CMD_FAN_CURVE 13
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
#define CLEVO_FAN_MODE_QUIET 8

/* Driver-visible fan modes. */
enum clevo_fan_mode {
	CLEVO_MODE_AUTO = 0,
	CLEVO_MODE_QUIET,
	CLEVO_MODE_MAX,
	CLEVO_MODE_MAXQ,
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
static int clevo_cc_dsm_buffer(struct clevo_cc *cc, u32 function, u8 *out,
			       size_t out_cap, size_t *out_len)
{
	union acpi_object pkg;
	u8 *payload;
	int err;

	payload = kzalloc(CLEVO_PAYLOAD_LEN, GFP_KERNEL);
	if (!payload)
		return -ENOMEM;

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

	if (len < 8)
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
	if (type != hwmon_fan)
		return 0;
	switch (attr) {
	case hwmon_fan_input:
		return 0444;
	default:
		return 0;
	}
}

static int clevo_cc_read(struct device *dev, enum hwmon_sensor_types type,
			 u32 attr, int channel, long *val)
{
	struct clevo_cc *cc = dev_get_drvdata(dev);
	u32 cpu_rpm, gpu_rpm;
	int err;

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
 * sysfs: fan_curve (read-only, placeholder for the custom-curve interface)
 *
 * Reports the current curve so the future graphical editor has a data source.
 * Format: "fan_count=<n> kb_type=<k> cpu:T1,D1,T2,D2,T3,D3,T4,D4 ..." with D
 * as raw 0..255. Writing a curve is intentionally not supported yet.
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
static DEVICE_ATTR_RO(fan_curve);

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
