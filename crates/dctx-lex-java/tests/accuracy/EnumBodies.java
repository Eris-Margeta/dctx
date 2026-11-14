enum Policy {
    KEEP { int apply(int n) { return n; } },
    DROP { int apply(int n) { return 0; } };
    abstract int apply(int n);
    static Policy fallback() { return KEEP; }
}
