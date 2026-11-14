class Factory {
    <T extends Number> Factory(T value) {}
    static <T> T identity(T value) { return value; }
    synchronized void update() {}
    native int query();
}
