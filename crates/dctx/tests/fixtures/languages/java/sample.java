@Deprecated
public class Boxed<T> {
    public <U> U copy(U value) {
        class Nested { int run() { return 1; } }
        return value;
    }
    public int café() { return 1; }
}
