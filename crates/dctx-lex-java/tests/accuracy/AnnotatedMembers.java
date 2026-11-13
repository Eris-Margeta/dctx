@SuppressWarnings({"unused", "class PhantomAnnotation {}"})
class Endpoint {
    @Deprecated
    private String address;
    @Override
    public String toString() { return address; }
}
