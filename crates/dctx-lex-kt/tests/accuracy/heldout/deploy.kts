#!/usr/bin/env kotlin

@file:DependsOn("com.example:tools:1.0")

import java.io.File

data class Target(val host: String, val port: Int = 22)

fun parseTargets(lines: List<String>): List<Target> =
    lines.filter { it.isNotBlank() }
        .map { line ->
            val (host, port) = line.split(":")
            Target(host, port.toInt())
        }

val targets = parseTargets(File("hosts.txt").readLines())

for (target in targets) {
    println("deploying to ${target.host} // fun phantomTemplate()")
}

class Report {
    fun print() = println("done")
}

Report().print()
