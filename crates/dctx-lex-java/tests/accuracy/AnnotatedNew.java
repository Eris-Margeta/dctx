import java.lang.annotation.*;
@Target(ElementType.TYPE_USE) @interface Mark {}
class Decorated {
    Runnable create() {
        return new @Mark Runnable() {
            public void run() {}
        };
    }
}
