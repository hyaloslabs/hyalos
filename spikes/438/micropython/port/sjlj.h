#ifndef HYALOS_SJLJ_H
#define HYALOS_SJLJ_H

// The bytes every live NLR buffer's snapshot shares (sjlj.c).
#define HY_SJLJ_ARENA (256 * 1024)

// Whether `run`'s call returned because the stack is unwinding for setjmp or longjmp.
int hy_sjlj_pending(void);
// Stops that unwind and starts the rewind it is for; `run` then calls its entry function again.
void hy_sjlj_resume(void);

#endif
