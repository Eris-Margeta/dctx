@deprecated
class Boxed<T> {
  U copy<U>(U value) {
    U nested(U item) { return item; }
    return nested(value);
  }
  int café() { return 1; }
}
