package platform

expect class PlatformFile(path: String) {
    val path: String
    fun read(): ByteArray
}

expect fun currentTimeMillis(): Long

actual object Clock {
    actual fun now(): Long = 0L
}

external fun nativeHash(data: ByteArray): Int

tailrec fun gcd(a: Int, b: Int): Int = if (b == 0) a else gcd(b, a % b)

@Suppress("NOTHING_TO_INLINE")
inline fun fast(): Int = 1
