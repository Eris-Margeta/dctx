class Selection {
    int choose(int n) {
        return switch (n) {
            case 0 -> { class Zero { int code() { return 0; } } yield new Zero().code(); }
            default -> 1;
        };
    }
}
