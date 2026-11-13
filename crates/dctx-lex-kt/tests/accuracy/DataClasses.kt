package shop.model

import java.time.Instant

data class Money(val cents: Long, val currency: String = "EUR") {
    operator fun plus(other: Money): Money = copy(cents = cents + other.cents)

    fun format(): String = "${cents / 100}.${cents % 100} $currency"
}

data class LineItem(
    val sku: String,
    val quantity: Int,
    val unitPrice: Money,
) {
    val total: Money
        get() = Money(unitPrice.cents * quantity, unitPrice.currency)
}

data class Order(val id: String, val items: List<LineItem>, val placedAt: Instant)
