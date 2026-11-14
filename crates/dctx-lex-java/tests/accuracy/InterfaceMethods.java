interface Channel {
    int LIMIT = 4;
    void send(String text);
    default void flush() { reset(); }
    static Channel empty() { return null; }
    private void reset() {}
    private static int capacity() { return LIMIT; }
}
