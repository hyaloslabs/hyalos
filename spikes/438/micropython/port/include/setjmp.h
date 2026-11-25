// setjmp and longjmp for MicroPython's NLR on plain WebAssembly, by Binaryen's Asyncify pass
// (sjlj.c). No exception-handling instructions and no imports: see ../sjlj.c.
#ifndef HYALOS_SETJMP_H
#define HYALOS_SETJMP_H

typedef struct hy_jmp_buf {
    int state;
    int value;
    // The Asyncify data this capture wrote: [start, top) in the snapshot arena.
    void *start;
    void *top;
    // The shadow stack pointer at the capture, restored when a longjmp returns here.
    void *sp;
} jmp_buf[1];

int hy_setjmp(struct hy_jmp_buf *env);
// Returns while the stack unwinds, so not `noreturn` (sjlj.c).
void hy_longjmp(struct hy_jmp_buf *env, int value);

#define setjmp(env) hy_setjmp(env)
#define longjmp(env, value) hy_longjmp(env, value)

#endif
