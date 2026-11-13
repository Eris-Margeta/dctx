class Ledger {
    int total() {
        record Line(int amount) {}
        return new Line(3).amount();
    }
}
