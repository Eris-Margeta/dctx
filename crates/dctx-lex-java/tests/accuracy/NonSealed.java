sealed class Event permits OpenEvent {}
non-sealed class OpenEvent extends Event {
    protected void dispatch() {}
}
