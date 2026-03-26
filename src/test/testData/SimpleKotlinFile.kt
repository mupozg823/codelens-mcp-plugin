package com.example

data class User(val id: Int, val name: String) {
    fun greet(): String = "Hello, $name"

    companion object {
        fun create(name: String): User = User(0, name)
    }
}

interface Repository<T> {
    fun findById(id: Int): T?
    fun save(item: T): T
}

object UserFactory {
    fun defaultUser(): User = User(1, "Default")
}

fun topLevelFunction(x: Int): Int = x * 2

val topLevelProperty: String = "hello"
