package com.codelens.standalone

import org.junit.Assert.*
import org.junit.Test
import java.nio.file.Files

class JetBrainsProxyTest {

    @Test
    fun `isAvailable returns false when no port file`() {
        val tmpDir = Files.createTempDirectory("proxy-test")
        val proxy = JetBrainsProxy(tmpDir)
        assertFalse(proxy.isAvailable())
        tmpDir.toFile().delete()
    }

    @Test
    fun `isAvailable returns false when port file has invalid content`() {
        val tmpDir = Files.createTempDirectory("proxy-test")
        Files.writeString(tmpDir.resolve(".codelens-port"), "not-a-number")
        val proxy = JetBrainsProxy(tmpDir)
        assertFalse(proxy.isAvailable())
        tmpDir.toFile().deleteRecursively()
    }

    @Test
    fun `isAvailable returns false when port is not listening`() {
        val tmpDir = Files.createTempDirectory("proxy-test")
        Files.writeString(tmpDir.resolve(".codelens-port"), "59999")
        val proxy = JetBrainsProxy(tmpDir)
        assertFalse(proxy.isAvailable())
        tmpDir.toFile().deleteRecursively()
    }

    @Test
    fun `readPort parses valid port file`() {
        val tmpDir = Files.createTempDirectory("proxy-test")
        Files.writeString(tmpDir.resolve(".codelens-port"), "24226")
        val proxy = JetBrainsProxy(tmpDir)
        assertEquals(24226, proxy.readPort())
        tmpDir.toFile().deleteRecursively()
    }
}
