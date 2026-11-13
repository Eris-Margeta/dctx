package tests.spec

import org.junit.Test

class `Calculator spec` {
    @Test
    fun `adds two numbers`() {
        check(1 + 1 == 2)
    }

    @Test
    fun `handles fun and class keywords in names`() {}

    val `in` = "keyword property"
}

fun `is`(value: Any) = value

object `object` {
    fun `fun`() = Unit
}
