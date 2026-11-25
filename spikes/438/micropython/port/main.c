// The hyalOS spike's MicroPython guest: `run`, the native `_hyalos` module, and the GC heap.
//
// Under a spike profile input record 0 is the contract, 1 the generated SDK and 2 the script
// (docs/plans/hyalos-kernel-python, design.md §2). `run` executes record 1 as a new module named
// `hyalos`, which MicroPython keeps in its loaded-modules dictionary (its `sys.modules`), so the
// script's `from hyalos import task` finds it. It then executes record 2 as `__main__`. Neither
// record's bytes are changed. An uncaught exception ends the run with `finish` of `error ` and the
// exception's type name.

#include <stdint.h>
#include <string.h>

#include "py/builtin.h"
#include "py/compile.h"
#include "py/gc.h"
#include "py/lexer.h"
#include "py/mperrno.h"
#include "py/runtime.h"
#include "py/stackctrl.h"
#include "sjlj.h"

#define HOST(name) __attribute__((import_module("hyalos"), import_name(name)))
HOST("read") int host_read(int index, int ptr, int cap);
HOST("infer") int host_infer(int schema, int ptr, int len);
HOST("effect") int host_effect(int kind, int ptr, int len);
HOST("return") void host_return(int ptr, int len);

// The GC heap. The collector never frees (gc_collect below), so this is all a run may allocate.
#define HEAP_SIZE (2 * 1024 * 1024)
// A data record's buffer, the digest guest's (hyalos-os/guest/src/lib.rs:28).
#define RECORD_CAP 16384
// Records 1 and 2, the SDK and the script.
#define SOURCE_CAP 16384
// MicroPython's own stack check, inside the linker's 512 KiB shadow stack (build.sh).
#define STACK_LIMIT (384 * 1024)

static uint8_t heap[HEAP_SIZE] __attribute__((aligned(16)));
static uint8_t record[RECORD_CAP];
static uint8_t sdk[SOURCE_CAP];
static uint8_t script[SOURCE_CAP];

static void finish(const void *bytes, size_t len) {
    host_return((int)(uintptr_t)bytes, (int)len);
    __builtin_unreachable(); // the host does not return from `return`
}

static void finish_error(const char *name, size_t len) {
    static uint8_t out[6 + 64];
    memcpy(out, "error ", 6);
    if (len > 64) {
        len = 64;
    }
    memcpy(out + 6, name, len);
    finish(out, 6 + len);
}

// ---- _hyalos -------------------------------------------------------------------------------

static mp_obj_t hyalos_read(mp_obj_t index_in) {
    mp_int_t index = mp_obj_get_int(index_in);
    int n = host_read((int)index, (int)(uintptr_t)record, RECORD_CAP);
    if (n == -1) {
        return mp_const_none;
    }
    if (n < 0) {
        // Too large for the buffer (-2): empty bytes, which no real record is.
        return mp_obj_new_bytes(record, 0);
    }
    return mp_obj_new_bytes(record, (size_t)n);
}
static MP_DEFINE_CONST_FUN_OBJ_1(hyalos_read_obj, hyalos_read);

static mp_obj_t hyalos_infer(mp_obj_t schema_in, mp_obj_t question_in) {
    mp_int_t schema = mp_obj_get_int(schema_in);
    mp_buffer_info_t question;
    mp_get_buffer_raise(question_in, &question, MP_BUFFER_READ);
    int label = host_infer((int)schema, (int)(uintptr_t)question.buf, (int)question.len);
    return MP_OBJ_NEW_SMALL_INT(label);
}
static MP_DEFINE_CONST_FUN_OBJ_2(hyalos_infer_obj, hyalos_infer);

static mp_obj_t hyalos_effect(mp_obj_t kind_in, mp_obj_t body_in) {
    mp_int_t kind = mp_obj_get_int(kind_in);
    mp_buffer_info_t body;
    mp_get_buffer_raise(body_in, &body, MP_BUFFER_READ);
    // A refusal ends the run in the monitor: this call then never returns.
    host_effect((int)kind, (int)(uintptr_t)body.buf, (int)body.len);
    return mp_const_none;
}
static MP_DEFINE_CONST_FUN_OBJ_2(hyalos_effect_obj, hyalos_effect);

static mp_obj_t hyalos_finish(mp_obj_t result_in) {
    mp_buffer_info_t result;
    mp_get_buffer_raise(result_in, &result, MP_BUFFER_READ);
    finish(result.buf, result.len);
    return mp_const_none;
}
static MP_DEFINE_CONST_FUN_OBJ_1(hyalos_finish_obj, hyalos_finish);

static const mp_rom_map_elem_t hyalos_globals_table[] = {
    { MP_ROM_QSTR(MP_QSTR___name__), MP_ROM_QSTR(MP_QSTR__hyalos) },
    { MP_ROM_QSTR(MP_QSTR_read), MP_ROM_PTR(&hyalos_read_obj) },
    { MP_ROM_QSTR(MP_QSTR_infer), MP_ROM_PTR(&hyalos_infer_obj) },
    { MP_ROM_QSTR(MP_QSTR_effect), MP_ROM_PTR(&hyalos_effect_obj) },
    { MP_ROM_QSTR(MP_QSTR_finish), MP_ROM_PTR(&hyalos_finish_obj) },
};
static MP_DEFINE_CONST_DICT(hyalos_globals, hyalos_globals_table);

const mp_obj_module_t hyalos_native_module = {
    .base = { &mp_type_module },
    .globals = (mp_obj_dict_t *)&hyalos_globals,
};
MP_REGISTER_MODULE(MP_QSTR__hyalos, hyalos_native_module);

// ---- run -----------------------------------------------------------------------------------

// Reads source record `index` into `buf`: its length, or a finish of `error BufferTooSmall`.
static size_t read_source(int index, uint8_t *buf) {
    int n = host_read(index, (int)(uintptr_t)buf, SOURCE_CAP);
    if (n == -2) {
        finish_error("BufferTooSmall", 14);
    }
    if (n < 0) {
        finish_error("NoSource", 8);
    }
    return (size_t)n;
}

static void execute(qstr name, const uint8_t *src, size_t len, mp_obj_dict_t *globals) {
    mp_lexer_t *lex = mp_lexer_new_from_str_len(name, (const char *)src, len, 0);
    mp_parse_compile_execute(lex, MP_PARSE_FILE_INPUT, globals, globals);
}

// `run`'s body. Asyncify instruments it, so a rewind (sjlj.c) re-enters it where it left off.
void hy_main(void) {
    mp_stack_ctrl_init();
    mp_stack_set_limit(STACK_LIMIT);
    gc_init(heap, heap + sizeof(heap));
    mp_init();

    nlr_buf_t nlr;
    if (nlr_push(&nlr) == 0) {
        size_t sdk_len = read_source(1, sdk);
        size_t script_len = read_source(2, script);
        mp_obj_t module = mp_obj_new_module(MP_QSTR_hyalos);
        execute(MP_QSTR_hyalos, sdk, sdk_len, ((mp_obj_module_t *)MP_OBJ_TO_PTR(module))->globals);
        execute(MP_QSTR___main__, script, script_len, mp_globals_get());
        nlr_pop();
        // The script ended without `finish`: the run ends with no result.
    } else {
        qstr name = mp_obj_get_type(MP_OBJ_FROM_PTR(nlr.ret_val))->name;
        size_t len;
        const char *text = (const char *)qstr_data(name, &len);
        finish_error(text, len);
    }
}

// The guest's one export. Not instrumented (build.sh): each setjmp or longjmp unwinds to here.
__attribute__((export_name("run"))) void run(void) {
    // Called through a volatile pointer: C cannot see that hy_main returns while the stack unwinds,
    // and a hy_main it judged `noreturn` would compile this loop away.
    void (*volatile entry)(void) = hy_main;
    for (;;) {
        entry();
        if (!hy_sjlj_pending()) {
            return;
        }
        hy_sjlj_resume();
    }
}

// ---- What the core asks of a port ---------------------------------------------------------

// The collector never frees. WebAssembly locals are not in linear memory, so a conservative scan
// of the shadow stack would miss pointers held only in them and free live objects. A run that
// fills the heap ends with MemoryError, the same way on every replay.
void gc_collect(void) {
}

// `print` and friends write nowhere: a run's only output is its effects and its result.
void mp_hal_stdout_tx_strn(const char *str, mp_uint_t len) {
    (void)str;
    (void)len;
}

void mp_hal_stdout_tx_strn_cooked(const char *str, size_t len) {
    (void)str;
    (void)len;
}

mp_lexer_t *mp_lexer_new_from_file(qstr filename) {
    (void)filename;
    mp_raise_OSError(MP_ENOENT);
}

mp_import_stat_t mp_import_stat(const char *path) {
    (void)path;
    return MP_IMPORT_STAT_NO_EXIST;
}

// An exception raised with no handler at all: none is ever outside hy_main's.
MP_NORETURN void nlr_jump_fail(void *val) {
    (void)val;
    finish_error("NlrJumpFail", 11);
    __builtin_unreachable(); // finish_error ends the run
}
