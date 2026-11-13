record Envelope<T extends CharSequence>(T value) {
    public Envelope { java.util.Objects.requireNonNull(value); }
    public <R> R map(java.util.function.Function<T, R> f) {
        return f.apply(value);
    }
}
