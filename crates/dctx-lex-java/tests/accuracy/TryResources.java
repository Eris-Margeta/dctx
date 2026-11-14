class Reader {
    void read() throws java.io.IOException {
        try (var input = java.nio.file.Files.newInputStream(java.nio.file.Path.of("x"))) {
            class Chunk { int length() { return 1; } }
            new Chunk().length();
        } catch (java.io.IOException failure) { throw failure; }
    }
}
