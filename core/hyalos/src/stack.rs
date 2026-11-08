//! The thread every command runs on (#280).
//!
//! Cedar checks how much stack is left before it evaluates or typechecks, and refuses when it looks
//! too little. On musl, which the static release binary uses, the main thread's stack is reported as
//! only the part mapped so far, so Cedar refuses on the main thread. Every request then answers
//! `unknown`, and no job can run. A spawned thread reports the stack it was given, so the binary
//! runs each command on one with glibc's default main-thread stack.

/// The stack a command runs with: what glibc gives a main thread.
pub const SIZE: usize = 8 << 20;

/// Runs `command` on a thread with [`SIZE`] of stack, and returns what it returns. A panic in it
/// carries on as a panic here, so the process ends as it would have without the thread.
pub fn run<T: Send + 'static>(command: impl FnOnce() -> T + Send + 'static) -> std::io::Result<T> {
    let thread = std::thread::Builder::new()
        .name("hyalos".to_string())
        .stack_size(SIZE)
        .spawn(command)?;
    Ok(thread
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic)))
}
