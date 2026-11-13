class Container<T> {
    class Item<U> { U value; }
}
class Allocation {
    Object create(Container<String> owner) {
        return owner.new Item<Integer>() {
            int size() { return 1; }
        };
    }
}
