class Listener {
    final Runnable callback = new Runnable() {
        private int calls;
        public void run() { calls++; }
    };
    int size() { return 1; }
}
