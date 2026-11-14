package tree

class Outer {
    private val secret = 42

    class Nested {
        fun hello() = "nested"

        class Deeper {
            val depth = 3
        }
    }

    inner class Inner {
        fun reveal() = secret
    }

    private enum class Mode { ON, OFF }

    interface Listener {
        fun onEvent()
    }
}

class Graph {
    data class Edge(val from: Int, val to: Int)

    object Empty
}
