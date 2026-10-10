#!/usr/bin/env bash
# Compile the real clevo_cc_gcmd_read source against a small ACPI mock and
# verify the exact Package { Buffer(256) } object passed to _DSM.

set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC="${1:-$ROOT/kernel/clevo-cc/clevo-cc.c}"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

awk '/^static int clevo_cc_gcmd_read\(/,/^}/' "$SRC" >"$tmp/function.c"
cat >"$tmp/test.c" <<'EOF'
#include <errno.h>
#include <stdint.h>
#include <stdio.h>

typedef uint8_t u8;
typedef uint32_t u32;
#define CLEVO_PAYLOAD_LEN 256
#define ACPI_TYPE_BUFFER 3
#define ACPI_TYPE_PACKAGE 4

union acpi_object {
    u32 type;
    struct { u32 type; u32 length; u8 *pointer; } buffer;
    struct { u32 type; u32 count; union acpi_object *elements; } package;
};
struct clevo_cc { int unused; };

static int injected_error;
static int clevo_cc_dsm_call(struct clevo_cc *cc, u32 command,
                             union acpi_object *arg3, u8 *out,
                             size_t out_cap, size_t *out_len, u32 *integer)
{
    u32 i;
    (void)cc;
    if (command != 122 || arg3->package.type != ACPI_TYPE_PACKAGE ||
        arg3->package.count != 1 || !arg3->package.elements ||
        arg3->package.elements[0].buffer.type != ACPI_TYPE_BUFFER ||
        arg3->package.elements[0].buffer.length != CLEVO_PAYLOAD_LEN ||
        !arg3->package.elements[0].buffer.pointer || out != NULL ||
        out_cap != 0 || !out_len || !integer)
        return -EINVAL;
    for (i = 0; i < CLEVO_PAYLOAD_LEN; i++)
        if (arg3->package.elements[0].buffer.pointer[i] != 0)
            return -EINVAL;
    *out_len = 0;
    *integer = 0x00900011;
    return injected_error;
}

EOF
cat "$tmp/function.c" >>"$tmp/test.c"
cat >>"$tmp/test.c" <<'EOF'

int main(void)
{
    struct clevo_cc cc = { 0 };
    u32 value = 0;
    int err = clevo_cc_gcmd_read(&cc, 122, &value);
    if (err || value != 0x00900011) {
        fprintf(stderr, "valid GCMD call failed: err=%d value=0x%08x\n", err, value);
        return 1;
    }
    injected_error = -EIO;
    err = clevo_cc_gcmd_read(&cc, 122, &value);
    if (err != -EIO) {
        fprintf(stderr, "GCMD error was not propagated: %d\n", err);
        return 1;
    }
    puts("kernel GCMD argument checks passed");
    return 0;
}
EOF

cc -std=c11 -Wall -Wextra -Werror "$tmp/test.c" -o "$tmp/test"
"$tmp/test"
