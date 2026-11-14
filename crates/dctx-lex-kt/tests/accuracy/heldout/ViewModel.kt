package ui

import kotlin.properties.Delegates

class CounterViewModel(
    private val start: Int,
    val label: String = "count",
) {
    var count: Int by Delegates.observable(start) { _, old, new ->
        log("changed $old -> $new")
    }
        private set

    val doubled by lazy { count * 2 }

    lateinit var listener: (Int) -> Unit

    constructor() : this(0)

    constructor(label: String) : this(0, label)

    fun increment() {
        fun bump(by: Int) {
            count += by
        }
        bump(1)
        listener(count)
    }

    private fun log(message: String) = println("[$label] $message")

    inner class Snapshot(val at: Long) {
        fun value() = count
    }
}
