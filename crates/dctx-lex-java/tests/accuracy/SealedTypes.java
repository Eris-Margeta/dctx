sealed interface Outcome permits Success, Failure {
    String message();
}
record Success(String message) implements Outcome {}
final class Failure implements Outcome {
    public String message() { return "failed"; }
}
