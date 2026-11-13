import java.lang.annotation.*;
@Target(ElementType.TYPE_USE) @interface Dimension {}
class Grids {
    String names @Dimension [];
    int cells @Dimension [] @Dimension [];
    void finish() {}
}
