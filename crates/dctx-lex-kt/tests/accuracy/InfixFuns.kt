package dsl.infix

infix fun Int.pow(exp: Int): Long {
    var result = 1L
    repeat(exp) { result *= this }
    return result
}

data class Pair2<A, B>(val first: A, val second: B)

infix fun <A, B> A.with(other: B) = Pair2(this, other)

class Matcher(val value: String) {
    infix fun shouldBe(expected: String) {
        check(value == expected) { "$value != $expected" }
    }

    private infix fun and(other: Matcher) = Matcher(value + other.value)
}

val sample = 2 pow 10
