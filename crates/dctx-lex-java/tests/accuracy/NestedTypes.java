class Registry {
    static class Entry {
        String key;
    }
    interface Factory { Entry create(); }
    enum State { READY, CLOSED }
    record Id(long value) {}
}
