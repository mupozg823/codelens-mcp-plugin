package com.codelens.standalone

import org.junit.Assert.*
import org.junit.Test
import java.nio.file.Files

class ProjectRootDetectorTest {

    @Test
    fun `detects git root from subdirectory`() {
        val tmpDir = Files.createTempDirectory("detect-test")
        val gitDir = tmpDir.resolve(".git")
        Files.createDirectory(gitDir)
        val sub = tmpDir.resolve("src/main/kotlin")
        Files.createDirectories(sub)

        val detected = ProjectRootDetector.detect(sub)
        assertEquals(tmpDir, detected)
        tmpDir.toFile().deleteRecursively()
    }

    @Test
    fun `returns cwd when no git root found`() {
        val tmpDir = Files.createTempDirectory("no-git-test")
        val detected = ProjectRootDetector.detect(tmpDir)
        assertEquals(tmpDir, detected)
        tmpDir.toFile().delete()
    }

    @Test
    fun `detects project yml as root marker`() {
        val tmpDir = Files.createTempDirectory("yml-test")
        val serenaDir = tmpDir.resolve(".serena")
        Files.createDirectory(serenaDir)
        Files.writeString(serenaDir.resolve("project.yml"), "project_name: test")
        val sub = tmpDir.resolve("src")
        Files.createDirectory(sub)

        val detected = ProjectRootDetector.detect(sub)
        assertEquals(tmpDir, detected)
        tmpDir.toFile().deleteRecursively()
    }
}
