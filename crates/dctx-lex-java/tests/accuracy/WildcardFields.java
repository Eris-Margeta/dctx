import java.util.*;
class Index {
    Map<String, List<? extends Number>> entries;
    List<? super Integer> sink;
    <T> List<T> copy(List<? extends T> source) { return new ArrayList<T>(source); }
}
