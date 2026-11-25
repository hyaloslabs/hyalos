# The hyalOS spike's MicroPython port for wasm32-unknown-unknown, with no Emscripten and no libc.
# build.sh copies this directory to ports/hyalos in a MicroPython checkout and runs `make` there,
# inside its toolchain container; see ../README.md.

include ../../py/mkenv.mk

MICROPY_ROM_TEXT_COMPRESSION = 0

include $(TOP)/py/py.mk

CC = clang-18
CPP = clang-18 -E --target=wasm32-unknown-unknown -nostdinc -isystem include -isystem $(shell clang-18 -print-resource-dir)/include
LD = wasm-ld-18
SIZE = true

CFLAGS += --target=wasm32-unknown-unknown -mcpu=mvp
CFLAGS += -nostdinc -isystem include -isystem $(shell clang-18 -print-resource-dir)/include
CFLAGS += -ffreestanding -fno-builtin -nostdlib
CFLAGS += -I. -I$(TOP) -I$(BUILD)
CFLAGS += -std=c99 -Wall -Os -DNDEBUG
CFLAGS += -fdata-sections -ffunction-sections
# Reproducible: no paths from the machine in the output.
CFLAGS += -ffile-prefix-map=$(abspath $(TOP))=/micropython

# No --allow-undefined: only functions declared with an import module (main.c, sjlj.c) are imports,
# so any other undefined symbol fails the link instead of becoming an import.
# A 512 KiB shadow stack first, then data; 4 MiB of memory in all, fixed (the profile's limit).
# wasm-ld exports the memory as `memory` itself.
LDFLAGS = --no-entry --stack-first -z stack-size=524288 \
	--initial-memory=4194304 --max-memory=4194304 \
	--gc-sections

SRC_C = main.c sjlj.c shared/libc/string0.c
SRC_QSTR += main.c

OBJ += $(PY_CORE_O)
OBJ += $(addprefix $(BUILD)/, $(SRC_C:.c=.o))

all: $(BUILD)/micropython.raw.wasm

$(BUILD)/micropython.raw.wasm: $(OBJ)
	$(ECHO) "LINK $@"
	$(Q)$(LD) $(LDFLAGS) -o $@ $^

include $(TOP)/py/mkrules.mk
