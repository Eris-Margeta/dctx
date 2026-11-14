package views

class View {
    val tag: String
    var width = 0

    constructor(tag: String) {
        this.tag = tag
    }

    constructor(tag: String, width: Int) : this(tag) {
        this.width = width
    }
}

class Label(val text: String) {
    constructor(number: Int) : this(number.toString())

    @Suppress("unused")
    private constructor() : this("")
}

class Failure : Exception {
    constructor(message: String) : super(message)
    constructor(cause: Throwable) : super(cause)
}
