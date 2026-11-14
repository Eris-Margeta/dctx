record Range(int low, int high) {
    Range {
        if (low > high) throw new IllegalArgumentException();
    }
    int width() { return high - low; }
}
