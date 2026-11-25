// No clock and no interrupts: a run's only inputs are its records and the host's answers.
static inline mp_uint_t mp_hal_ticks_ms(void) {
    return 0;
}
static inline void mp_hal_set_interrupt_char(char c) {
    (void)c;
}
