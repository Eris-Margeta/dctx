package billing

sealed interface PaymentResult {
    data class Paid(val receipt: String, val amountCents: Long) : PaymentResult
    data object Pending : PaymentResult
    sealed class Failed(open val reason: String) : PaymentResult {
        class Declined(override val reason: String) : Failed(reason)
        class Network(override val reason: String, val retryAfter: Long) : Failed(reason)
    }
}

enum class Currency(val symbol: String) {
    EUR("€") {
        override fun format(cents: Long) = "$symbol${cents / 100}"
    },
    USD("$") {
        override fun format(cents: Long) = "${symbol}${cents / 100}"
    };

    abstract fun format(cents: Long): String

    companion object {
        fun parse(code: String): Currency = valueOf(code)
    }
}

fun PaymentResult.describe(): String = when (this) {
    is PaymentResult.Paid -> "paid $receipt"
    PaymentResult.Pending -> "class PhantomPending"
    is PaymentResult.Failed -> reason
}
