#!/usr/bin/env kotlin

import java.io.File

val root = File(".")
var count = 0

fun walk(dir: File): Int = dir.listFiles()?.size ?: 0

data class Entry(val name: String, val size: Long)

root.listFiles()?.forEach { file ->
    val local = file.name
    count++
    println("$local fun phantomPrint()")
}

class Summary {
    fun print() = println("files: $count")
}

Summary().print()
