package ui.theme

enum class Direction { NORTH, EAST, SOUTH, WEST }

enum class Planet(val mass: Double, val radius: Double) {
    MERCURY(3.303e+23, 2.4397e6),
    EARTH(5.976e+24, 6.37814e6);

    val surfaceGravity: Double
        get() = 6.67300E-11 * mass / (radius * radius)

    companion object {
        fun heaviest(): Planet = values().maxBy { it.mass }
    }
}

enum class Op {
    ADD {
        override fun apply(a: Int, b: Int) = a + b
    },
    MUL {
        override fun apply(a: Int, b: Int) = a * b
    };

    abstract fun apply(a: Int, b: Int): Int
}
