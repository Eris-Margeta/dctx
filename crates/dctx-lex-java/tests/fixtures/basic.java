/**
 * Sample types for café.
 */
package com.example.app;

// class Ghost {}

import java.util.List;
import java.util.*;
import static java.util.Collections.emptyList;
import static java.util.Objects.*;

/**
 * A box of values.
 */
public class Box<T extends CharSequence> extends Base implements Runnable {
    /** Capacity. */
    public final int capacity = 2;

    private List<String> names;

    /**
     * Builds a box.
     */
    public Box(int capacity) {
        this.capacity = capacity;
        store.load(names);
    }

    @Override
    public void run() {
        service.run(emptyList());
    }

    /** Nested holder. */
    public static class Nested {
        void ping() { pingBack(); }
        void ping(int times) { pingBack(); }
    }
}

/** One point. */
record Point(int x, String label) implements Marker {
    public int doubled() { return x; }
}

enum Color {
    RED,
    GREEN,
    BLUE {
        int code() { return 1; }
    };

    private final int n;

    Color(int n) { this.n = n; }
}

interface Task {
    void run();
}

@FunctionalInterface
interface Marker {}

@interface Tag {
    String value() default "x";
}

class App {
    private String greeting = """
        class NotAClass { }
        /* not a comment */
        """;

    private char mark = '"';

    private String tricky = "class Fake { void no() {} }";

    void start() {
        Runnable r = new Runnable() {
            public void run() { hook(); }
        };
        names.forEach(name -> sink.accept(name));
        Point p = new Point(1, "a");
        Store.find(id);
        int ignored = point.x;
        Consumer<String> c = String::trim;
    }

    @Test
    void saves() { repo.save(id); }
}
