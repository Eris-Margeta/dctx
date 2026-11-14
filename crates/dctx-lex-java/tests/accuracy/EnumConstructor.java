enum Level {
    @Deprecated LOW(1), HIGH(2);
    private final int weight;
    Level(int weight) { this.weight = weight; }
    int weight() { return weight; }
}
