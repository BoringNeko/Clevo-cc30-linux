/* The emulator checks the firmware payload, state confirmation and recovery. */
#include <assert.h>
#include <errno.h>
#include <limits.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/types.h>
typedef uint8_t u8;
#define CLEVO_PAYLOAD_LEN 256
#define CLEVO_CMD_SETTINGS 4
#define CLEVO_SUB_FLEXICHARGER_READ 30
#define CLEVO_SUB_FLEXICHARGER_WRITE 31
#define CLEVO_CHARGE_VALUES_LEN 16
struct clevo_cc { int charge_lock; };
#define dev_err(...) do { restore_errors++; } while (0)
#include "window.c"
static u8 firmware[256];
static int writes, failure, restore_errors, short_reply, read_error;
static int clevo_cc_dsm_payload(struct clevo_cc *cc, unsigned int command,
                               const u8 *payload, u8 *out, size_t cap, size_t *len)
{
    int i;
    (void)cc;
    assert(command == 4 && cap == 256);
    if (payload[0] == 30) {
        for (i = 1; i < 256; i++) assert(payload[i] == 0);
        memcpy(out, firmware, 256);
        *len = short_reply ? 47 : 256;
        return read_error;
    }
    assert(payload[0] == 31);
    for (i = 4; i < 256; i++) assert(payload[i] == 0);
    writes++;
    memcpy(firmware, payload + 1, 3);
    memset(out, 0, 256);
    *len = 256;
    if (failure == 1 && writes == 1) firmware[2] = 80; /* clamped */
    if (failure == 2 && writes == 1) return -EIO; /* changed before error */
    if (failure == 3) return -EIO; /* recovery also fails */
    if (failure == 4 && writes == 1) *len = 0; /* integer acknowledgement */
    return 0;
}
#include "charge.c"

/* Run the real sysfs store callback, not just the decoded command helper. */
#define GFP_KERNEL 0
#define ACPI_ALLOCATE_BUFFER 0
struct acpi_buffer { size_t length; void *pointer; };
struct device { struct clevo_cc *cc; };
struct device_attribute { int unused; };
static void *dev_get_drvdata(struct device *dev) { return dev->cc; }
static void kfree(void *memory) { free(memory); }
static void mutex_lock(int *lock) { assert(!*lock); *lock = 1; }
static void mutex_unlock(int *lock) { assert(*lock); *lock = 0; }
static int clevo_cc_charge_battery(struct clevo_cc *cc, struct acpi_buffer *name)
{
    (void)cc;
    name->pointer = malloc(1);
    assert(name->pointer);
    return 0;
}
static char **argv_split(int flags, const char *text, int *argc)
{
    char **words = calloc(strlen(text) + 2, sizeof(*words));
    char *copy = malloc(strlen(text) + 1), *token;
    (void)flags;
    assert(words && copy);
    strcpy(copy, text);
    /* Keep the allocation before the returned argv to also handle empty input. */
    words[0] = copy;
    *argc = 0;
    for (token = strtok(copy, " \t\r\n"); token; token = strtok(NULL, " \t\r\n"))
        words[++*argc] = token;
    return words + 1;
}
static void argv_free(char **words) { free(words[-1]); free(words - 1); }
static int kstrtouint(const char *text, unsigned int base, unsigned int *value)
{
    unsigned long parsed;
    char *end;
    if (!*text || *text == '-') return -EINVAL;
    errno = 0;
    parsed = strtoul(text, &end, base);
    if (errno == ERANGE || parsed > UINT_MAX) return -ERANGE;
    if (*end) return -EINVAL;
    *value = (unsigned int)parsed;
    return 0;
}
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Wunused-parameter"
#pragma GCC diagnostic ignored "-Wsign-compare"
#include "store.c"
#pragma GCC diagnostic pop

static void reset(void)
{
    const u8 start[] = {40, 50, 60, 70, 80, 95};
    const u8 end[] = {60, 70, 80, 90, 100};
    memset(firmware, 0, sizeof(firmware));
    firmware[0] = 0; firmware[1] = 95; firmware[2] = 100;
    memcpy(firmware + 16, start, sizeof(start));
    memcpy(firmware + 32, end, sizeof(end));
    writes = failure = restore_errors = short_reply = read_error = 0;
}
int main(void)
{
    struct clevo_cc cc = {0};
    struct device dev = {&cc};
    struct clevo_charge_window window;
    int i;
    reset();
    assert(!clevo_cc_charge_read(&cc, &window) && !window.enabled);
    assert(!writes);
    assert(!clevo_cc_charge_set(&cc, 1, 40, 80));
    assert(firmware[0] == 1 && firmware[1] == 40 && firmware[2] == 80);
    assert(writes == 1);
    assert(!clevo_cc_charge_set(&cc, 1, 40, 80) && writes == 1);
    assert(!clevo_cc_charge_set(&cc, 0, 40, 80) && writes == 2);
    assert(!firmware[0]);
    reset();
    assert(clevo_cc_charge_set(&cc, 2, 40, 80) == -EINVAL);
    assert(clevo_cc_charge_set(&cc, 1, 50, 85) == -EINVAL);
    assert(clevo_cc_charge_set(&cc, 1, 41, 80) == -EINVAL);
    assert(clevo_cc_charge_set(&cc, 1, 80, 80) == -EINVAL);
    assert(clevo_cc_charge_set(&cc, 1, 40, 256) == -EINVAL);
    assert(!writes);
    short_reply = 1;
    assert(clevo_cc_charge_set(&cc, 1, 40, 80) == -EPROTO && !writes);
    short_reply = 0; read_error = -EIO;
    assert(clevo_cc_charge_set(&cc, 1, 40, 80) == -EIO && !writes);
    reset(); memset(firmware, 0, sizeof(firmware));
    assert(clevo_cc_charge_read(&cc, &window) == -EOPNOTSUPP);
    reset(); firmware[16] = 101;
    assert(clevo_cc_charge_read(&cc, &window) == -EOPNOTSUPP);
    reset(); firmware[18] = 0;
    assert(clevo_cc_charge_read(&cc, &window) == -EOPNOTSUPP);
    reset(); firmware[0] = 2;
    assert(clevo_cc_charge_read(&cc, &window) == -EPROTO);
    reset(); firmware[1] = 41;
    assert(clevo_cc_charge_read(&cc, &window) == -EPROTO);
    for (i = 1; i <= 4; i++) {
        reset(); failure = i;
        assert(clevo_cc_charge_set(&cc, 1, 50, 90) == (i == 4 ? -EPROTO : -EIO));
        assert(writes == 2);
        assert(!firmware[0] && firmware[1] == 95 && firmware[2] == 100);
        assert(restore_errors == (i == 3 ? 1 : 0));
    }
    reset();
    {
        const char *valid[] = {"1 40 80\n", "0 95 100\n", " 1\t50 90 \n"};
        const char *invalid[] = {
            "1", "1 40", " ", "\n", "", "1 40 80 extra", "1 40 80 0",
            "1 41 80", "1 40 85", "2 40 80", "1 80 80", "-1 40 80",
            "1 40 429496729680", "true 40 80", "1 40 80x",
        };
        size_t j;
        int before;
        for (j = 0; j < sizeof(valid) / sizeof(valid[0]); j++)
            assert(flexicharger_store(&dev, NULL, valid[j], strlen(valid[j])) == (ssize_t)strlen(valid[j]));
        assert(writes == 3);
        before = writes;
        for (j = 0; j < sizeof(invalid) / sizeof(invalid[0]); j++) {
            assert(flexicharger_store(&dev, NULL, invalid[j], strlen(invalid[j])) == -EINVAL);
            assert(!cc.charge_lock && writes == before);
        }
        read_error = -EIO;
        assert(flexicharger_store(&dev, NULL, "1 40 80\n", 8) == -EIO);
        assert(!cc.charge_lock && writes == before);
    }
    puts("kernel FlexiCharger sysfs parsing, payloads, discrete values and recovery passed");
    return 0;
}
