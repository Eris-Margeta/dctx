/**
 * Sample types.
 */
package com.example.app

import kotlin.collections.List
import kotlin.collections.*
import com.example.util.Names as Labels

// class Ghost

/**
 * A box of values.
 */
open class Box<T : CharSequence>(public val item: T) : Base() {
    /**
     * Size of the box.
     */
    val size: Int = item.length

    fun show(): String {
        return render(item)
    }
}

/** Holds a point. */
data class Point(val x: Int, val y: Int)

enum class Color {
    RED,
    GREEN {
        fun code(): Int = 1
    };
    val n: Int = 0
}

sealed interface Task {
    fun run()
}

object Registry {
    fun add(name: String) {
        store.save(name)
    }
}

typealias Id = String

fun <T> identity(value: T): T = value

fun String?.trimmed(): String = trim()

class App {
    private val greeting: String = """
        class NotAClass { }
        ${hidden()}
        """

    fun start() {
        box.show()
        Registry.add(greeting)
        identity(1)
        items.forEach { item -> item.go() }
    }
}
