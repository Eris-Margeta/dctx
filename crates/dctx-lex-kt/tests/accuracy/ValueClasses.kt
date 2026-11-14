package ids

@JvmInline
value class UserId(val raw: String) {
    init {
        require(raw.isNotBlank()) { "blank id" }
    }

    fun masked(): String = raw.take(2) + "***"
}

@JvmInline
value class Meters(val value: Double) : Comparable<Meters> {
    override fun compareTo(other: Meters): Int = value.compareTo(other.value)
}

inline class LegacyToken(val token: String)
