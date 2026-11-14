package math.vec

data class Vec(val x: Double, val y: Double) {
    operator fun plus(o: Vec) = Vec(x + o.x, y + o.y)
    operator fun minus(o: Vec) = Vec(x - o.x, y - o.y)
    operator fun times(k: Double) = Vec(x * k, y * k)
    operator fun unaryMinus() = Vec(-x, -y)
    operator fun get(index: Int): Double = if (index == 0) x else y
    operator fun component3(): Double = 0.0
}

operator fun Double.times(v: Vec) = v * this

class Grid(private val cells: IntArray, val width: Int) {
    operator fun get(row: Int, col: Int) = cells[row * width + col]

    operator fun set(row: Int, col: Int, value: Int) {
        cells[row * width + col] = value
    }

    operator fun contains(value: Int) = value in cells

    operator fun invoke(): Int = cells.sum()
}
