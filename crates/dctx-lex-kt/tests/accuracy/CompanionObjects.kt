package db

class Connection private constructor(val url: String) {
    companion object {
        const val DEFAULT_PORT = 5432
        private var opened = 0

        fun open(url: String): Connection {
            opened++
            return Connection(url)
        }
    }

    fun close() {}
}

class Pool {
    companion object Factory : (Int) -> Pool {
        override fun invoke(size: Int): Pool = Pool()
    }
}

interface Codec {
    companion object {
        val NONE: Codec = object : Codec {}
    }
}
