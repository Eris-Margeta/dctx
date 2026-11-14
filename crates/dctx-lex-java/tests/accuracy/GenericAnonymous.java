abstract class Holder<T> {
    abstract T value();
    static class Nested<U> { U item; }
}
class Instances {
    Object value = new Holder.Nested<String>() {
        String read() { return item; }
    };
}
