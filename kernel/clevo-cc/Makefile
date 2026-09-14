# SPDX-License-Identifier: GPL-2.0-only
#
# Out-of-tree build for the clevo-cc ACPI platform driver.
#
# Some distributions (e.g. CachyOS) build the kernel with Clang + ThinLTO and
# fail to link a GCC-built module against it. Detect that case and pass LLVM=1.
# Override explicitly with `make LLVM=1` or `make LLVM=` if detection is wrong.
#
# Build against the running kernel by default; cross-build against `KDIR` or the
# `KERNELRELEASE` DKMS variable otherwise.

obj-m += clevo-cc.o

KDIR ?= /lib/modules/$(shell uname -r)/build

# Auto-detect a Clang-built kernel when LLVM is not set on the command line.
# CONFIG_CC_IS_CLANG is present in the kernel .config shipped by most distros.
LLVM ?= $(shell \
	if [ -r "$(KDIR)/.config" ] && grep -q '^CONFIG_CC_IS_CLANG=y' "$(KDIR)/.config"; then \
		echo 1; \
	fi)

.PHONY: all clean

all:
	$(MAKE) -C $(KDIR) M=$(PWD) $(if $(LLVM),LLVM=$(LLVM)) modules

clean:
	$(MAKE) -C $(KDIR) M=$(PWD) clean
