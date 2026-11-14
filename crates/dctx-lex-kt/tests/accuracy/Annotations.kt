@file:JvmName("AnnotationsKt")
@file:Suppress("unused", "UNUSED_PARAMETER")

package meta

import kotlin.annotation.AnnotationTarget.CLASS

@Target(CLASS, AnnotationTarget.FUNCTION)
@Retention(AnnotationRetention.RUNTIME)
annotation class Route(val path: String, val methods: Array<String> = ["GET"])

annotation class Marker

@Route("/users/{id}", methods = ["GET", "DELETE"])
class UserController {
    @Deprecated("use fetch { class PhantomDeprecated }", ReplaceWith("fetch(id)"))
    fun get(id: String) = fetch(id)

    @Route(path = "/x")
    @Marker
    fun fetch(id: String): String = id

    @Volatile
    var hits: Int = 0
}

@Suppress("MagicNumber") val timeoutMs = 30_000
