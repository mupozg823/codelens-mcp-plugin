package com.codelens.standalone

import java.nio.file.Files
import java.nio.file.Path

object ProjectRootDetector {

    private val ROOT_MARKERS = listOf(
        ".git",
        ".serena/project.yml",
        "build.gradle.kts",
        "build.gradle",
        "package.json",
        "pyproject.toml",
        "Cargo.toml",
        "pom.xml"
    )

    fun detect(startDir: Path): Path {
        var current = startDir.toAbsolutePath().normalize()
        val home = Path.of(System.getProperty("user.home")).toAbsolutePath().normalize()

        while (current != current.root && current != home.parent) {
            for (marker in ROOT_MARKERS) {
                if (Files.exists(current.resolve(marker))) {
                    return current
                }
            }
            current = current.parent ?: break
        }
        return startDir.toAbsolutePath().normalize()
    }
}
