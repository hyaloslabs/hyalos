// setjmp and longjmp for MicroPython's NLR (py/nlrsetjmp.c) on plain WebAssembly.
//
// WebAssembly cannot jump between frames, and LLVM lowers setjmp only to the exception-handling
// proposal or to Emscripten's JavaScript, neither of which a hyalOS profile loads. This port uses
// Binaryen's Asyncify pass instead (build.sh runs it), which rewrites every function that can reach
// these two into one that can save its locals to memory and return ("unwind"), and later be called
// again to restore them and carry on from the same call ("rewind"). The result is plain
// WebAssembly: Asyncify's own functions become internal, and no import is added.
//
// - setjmp unwinds the whole stack to `run` (main.c), which keeps where the snapshot ended and
//   rewinds it at once; setjmp, called again by the rewind, returns 0.
// - longjmp unwinds the stack to `run`, which rewinds the snapshot of the buffer it names: every
//   frame from `run` to setjmp's caller is rebuilt as it was, and setjmp returns the value.
//
// Snapshots go in one arena. NLR buffers nest strictly (nlr_push and nlr_pop are last in, first
// out), so a new capture starts where the enclosing buffer's snapshot ends (`nlr_top->prev`), and a
// buffer's snapshot stays intact while it is live. An arena or snapshot that overflows traps, so a
// run that nests too deeply ends the same way on every replay.
//
// Functions that call Asyncify's start and stop functions directly are not instrumented
// themselves; their callers are (Binaryen's "bottom-most runtime" rule).

#include <stdint.h>
#include <setjmp.h>
#include "py/mpstate.h"
#include "sjlj.h"

#define IMPORT(name) __attribute__((import_module("asyncify"), import_name(name)))
IMPORT("start_unwind") void asyncify_start_unwind(void *data);
IMPORT("stop_unwind") void asyncify_stop_unwind(void);
IMPORT("start_rewind") void asyncify_start_rewind(void *data);
IMPORT("stop_rewind") void asyncify_stop_rewind(void);

__asm__(".globaltype __stack_pointer, i32");

static inline void *get_sp(void) {
    void *sp;
    __asm__ volatile ("global.get __stack_pointer\n\tlocal.set %0" : "=r" (sp));
    return sp;
}

static inline void set_sp(void *sp) {
    __asm__ volatile ("local.get %0\n\tglobal.set __stack_pointer" : : "r" (sp));
}

enum { FRESH = 0, CAPTURING, CAPTURED, RETURNING };

// Asyncify's data: the next free byte and the end.
static struct {
    uint8_t *cur;
    uint8_t *end;
} data;

static uint8_t arena[HY_SJLJ_ARENA] __attribute__((aligned(16)));
static struct hy_jmp_buf *active;

int hy_setjmp(struct hy_jmp_buf *env) {
    if (active == env) {
        // Called again by the rewind `hy_sjlj_resume` started.
        asyncify_stop_rewind();
        active = NULL;
        if (env->state == RETURNING) {
            env->state = CAPTURED;
            set_sp(env->sp);
            return env->value;
        }
        env->state = CAPTURED;
        return 0;
    }
    nlr_buf_t *prev = MP_STATE_THREAD(nlr_top) ? MP_STATE_THREAD(nlr_top)->prev : NULL;
    env->state = CAPTURING;
    env->start = prev ? prev->jmpbuf[0].top : arena;
    env->sp = get_sp();
    data.cur = env->start;
    data.end = arena + sizeof(arena);
    active = env;
    asyncify_start_unwind(&data);
    return -1; // not seen: the stack is unwinding
}

void hy_longjmp(struct hy_jmp_buf *env, int value) {
    env->state = RETURNING;
    env->value = value ? value : 1;
    // This unwind's data is thrown away: write it after the snapshot, which must stay intact.
    data.cur = env->top;
    data.end = arena + sizeof(arena);
    active = env;
    // Return, not trap: this function is not instrumented, and its caller, which is, sees the stack
    // unwinding and returns too, all the way to `run`.
    asyncify_start_unwind(&data);
}

int hy_sjlj_pending(void) {
    return active != NULL;
}

void hy_sjlj_resume(void) {
    asyncify_stop_unwind();
    if (active->state == CAPTURING) {
        active->top = data.cur;
    }
    data.cur = active->top;
    data.end = arena + sizeof(arena);
    asyncify_start_rewind(&data);
}
