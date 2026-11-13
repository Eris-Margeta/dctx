auto twice = [](int x) { return x * 2; };
int apply(int x) {
    auto adjust = [x](int delta) { return x + delta; };
    return adjust(twice(x));
}
