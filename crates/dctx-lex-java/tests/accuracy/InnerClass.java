class Session {
    private int count;
    class Cursor {
        Cursor() {}
        int next() { return ++count; }
    }
    Cursor cursor() { return new Cursor(); }
}
