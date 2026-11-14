import java.util.function.*;
class References {
    Function<String, String> trim = String::trim;
    Supplier<StringBuilder> build = StringBuilder::new;
    IntFunction<String[]> allocate = String[]::new;
    void accept() { trim.apply("value"); }
}
