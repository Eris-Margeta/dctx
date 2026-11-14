package net.handlers

typealias Headers = Map<String, List<String>>

typealias Handler<R> = suspend (Request) -> R

fun interface Middleware {
    fun wrap(next: Handler<Response>): Handler<Response>
}

@JvmInline
value class StatusCode(val code: Int) {
    val isError: Boolean get() = code >= 400
}

data class Request(val path: String, val headers: Headers = emptyMap())

data class Response(val status: StatusCode, val body: String = "")

object Router {
    private val routes = HashMap<String, Handler<Response>>()

    fun route(path: String, handler: Handler<Response>) {
        routes[path] = handler
    }

    suspend fun dispatch(request: Request): Response =
        routes[request.path]?.invoke(request)
            ?: Response(StatusCode(404), "no route; object PhantomRoute")
}

val logging = Middleware { next ->
    { request ->
        println("-> ${request.path}")
        next(request)
    }
}

const val DEFAULT_PORT: Int = 8080
