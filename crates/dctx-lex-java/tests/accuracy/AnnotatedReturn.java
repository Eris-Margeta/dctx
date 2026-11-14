import java.lang.annotation.*;
@Target(ElementType.TYPE_USE) @interface Axis {}
class Axes {
    String names() @Axis [] {
        class Label { String text; }
        return new String[0];
    }
    void done() {}
}
