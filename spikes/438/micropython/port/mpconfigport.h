// MicroPython configuration for the hyalOS spike port (spikes/438/micropython/README in ../).
// Built from MicroPython's minimum ROM level, adding only what the generated SDK and the triage
// script use. No floats, no filesystem, no time, no `sys` or other native module but `_hyalos`.

#include <stdint.h>

#define MICROPY_CONFIG_ROM_LEVEL                (MICROPY_CONFIG_ROM_LEVEL_MINIMUM)

#define MICROPY_ENABLE_COMPILER                 (1)
#define MICROPY_ENABLE_GC                       (1)
#define MICROPY_ERROR_REPORTING                 (MICROPY_ERROR_REPORTING_NONE)
#define MICROPY_NLR_SETJMP                      (1)
#define MICROPY_STACK_CHECK                     (1)
#define MICROPY_USE_INTERNAL_ERRNO              (1)

// Integers: arbitrary precision in C, so a long issue number is a number, not an error.
#define MICROPY_LONGINT_IMPL                    (MICROPY_LONGINT_IMPL_MPZ)
#define MICROPY_FLOAT_IMPL                      (MICROPY_FLOAT_IMPL_NONE)

// What the SDK and script use beyond the minimum: `bytearray` (the SDK names it), `str.encode`,
// slices.
#define MICROPY_PY_BUILTINS_BYTEARRAY           (1)
#define MICROPY_CPYTHON_COMPAT                  (1)
#define MICROPY_PY_BUILTINS_SLICE               (1)

// Nothing reaches outside the interpreter.
#define MICROPY_PY_SYS                          (0)
#define MICROPY_PY_MICROPYTHON                  (0)
#define MICROPY_PY_IO                           (0)
#define MICROPY_ENABLE_EXTERNAL_IMPORT          (0)
#define MICROPY_HELPER_REPL                     (0)
#define MICROPY_KBD_EXCEPTION                   (0)
#define MICROPY_GC_ALLOC_THRESHOLD              (0)

typedef long mp_off_t;
// No libc: the signed size type MicroPython's core uses.
typedef long ssize_t;

#define MICROPY_HW_BOARD_NAME                   "hyalos"
#define MICROPY_HW_MCU_NAME                     "wasm32"

#define MP_STATE_PORT                           MP_STATE_VM

#include <alloca.h>
