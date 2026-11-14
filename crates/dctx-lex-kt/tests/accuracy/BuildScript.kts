plugins {
    kotlin("jvm") version "2.0.0"
    application
}

group = "org.example"
version = "1.0-SNAPSHOT"

repositories {
    mavenCentral()
}

val ktorVersion: String by project

dependencies {
    implementation("io.ktor:ktor-server-core:$ktorVersion")
    testImplementation(kotlin("test"))
}

tasks.register("hello") {
    val greeting = "class PhantomTask"
    doLast { println(greeting) }
}

fun gitHash(): String = "unknown"

application {
    mainClass.set("org.example.MainKt")
}
