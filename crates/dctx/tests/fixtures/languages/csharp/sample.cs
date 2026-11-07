namespace Sample;
[Obsolete]
public class Boxed<T> {
    [Obsolete]
    public U Copy<U>(U value) {
        U Nested(U item) { return item; }
        return Nested(value);
    }
    public int café() { return 1; }
}
