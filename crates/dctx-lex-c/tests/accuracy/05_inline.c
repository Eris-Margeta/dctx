static inline unsigned rotate(unsigned value) {
    return (value << 1) | (value >> 31);
}
inline int is_ready(void) { return 1; }
