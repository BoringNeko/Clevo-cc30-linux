/* Test the actual extracted driver helpers, including failed firmware writes. */
#include <assert.h>
#include <errno.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>

typedef uint32_t u32;
#define BIT(n) (1UL << (n))
#define CLEVO_CMD_EXTENDED_FEATURES 96
#define CLEVO_CMD_SUPPORTED_APPS 16
#define CLEVO_SUB_PERF_MODE 25
#include "enums.c"
enum platform_profile_option {
    PLATFORM_PROFILE_LOW_POWER, PLATFORM_PROFILE_COOL,
    PLATFORM_PROFILE_QUIET, PLATFORM_PROFILE_BALANCED,
    PLATFORM_PROFILE_BALANCED_PERFORMANCE, PLATFORM_PROFILE_PERFORMANCE,
};
struct clevo_cc {
    enum clevo_perf_mode perf_mode;
    bool perf_mode_set;
    int perf_lock;
};
static u32 features = BIT(2), apps = BIT(0), last_mode;
static int query_error, write_error, writes;
static void mutex_lock(int *lock) { assert(!*lock); *lock = 1; }
static void mutex_unlock(int *lock) { assert(*lock); *lock = 0; }
static void set_bit(int bit, unsigned long *choices) { *choices |= BIT(bit); }
static int clevo_cc_gcmd_read(struct clevo_cc *cc, u32 command, u32 *value)
{
    (void)cc;
    assert(command == 96 || command == 16);
    *value = command == 96 ? features : apps;
    return query_error;
}
static int clevo_cc_main_cmd(struct clevo_cc *cc, u32 sub, u32 mode)
{
    assert(cc->perf_lock);
    assert(sub == 25 && mode < 4);
    writes++;
    last_mode = mode;
    return write_error;
}
#include "perf.c"
int main(void)
{
    struct clevo_cc cc = {0};
    enum platform_profile_option current;
    const enum platform_profile_option profiles[] = {
        PLATFORM_PROFILE_QUIET, PLATFORM_PROFILE_LOW_POWER,
        PLATFORM_PROFILE_PERFORMANCE, PLATFORM_PROFILE_BALANCED,
    };
    unsigned long choices = 0;
    int i, before;

    assert(clevo_cc_profile_get(&cc, &current) == -ENODATA);
    assert(writes == 0 && !cc.perf_mode_set);
    clevo_cc_profile_choices(&choices);
    for (i = 0; i < 4; i++) {
        assert(choices & BIT(profiles[i]));
        assert(!clevo_cc_profile_set(&cc, profiles[i]));
        assert(last_mode == (u32)i && cc.perf_mode == (enum clevo_perf_mode)i);
        assert(!clevo_cc_profile_get(&cc, &current) && current == profiles[i]);
    }
    before = writes;
    assert(clevo_cc_profile_set(&cc, PLATFORM_PROFILE_COOL) == -EOPNOTSUPP);
    assert(clevo_cc_set_perf_mode(&cc, 4) == -EINVAL);
    features = 0;
    assert(clevo_cc_profile_set(&cc, profiles[0]) == -EOPNOTSUPP);
    features = BIT(2); apps = 0;
    assert(clevo_cc_profile_set(&cc, profiles[0]) == -EOPNOTSUPP);
    apps = BIT(0); query_error = -EIO;
    assert(clevo_cc_profile_set(&cc, profiles[0]) == -EIO);
    assert(writes == before && cc.perf_mode == 3);
    query_error = 0; write_error = -EIO;
    assert(clevo_cc_profile_set(&cc, profiles[0]) == -EIO);
    assert(cc.perf_mode == 3 && cc.perf_mode_set && !cc.perf_lock);
    puts("kernel platform-profile mapping, capability gates and failure checks passed");
    return 0;
}
