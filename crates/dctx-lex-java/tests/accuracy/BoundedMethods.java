class Ordering<T extends Number & Comparable<T>> {
    <U extends CharSequence & Comparable<U>> U first(U a, U b) {
        return a.compareTo(b) < 0 ? a : b;
    }
    <E extends Exception> void raise(E error) throws E { throw error; }
}
