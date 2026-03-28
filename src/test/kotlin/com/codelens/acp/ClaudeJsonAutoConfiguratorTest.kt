package com.codelens.acp

import com.codelens.CodeLensTestBase
import java.nio.file.Files
import java.nio.file.Path

class ClaudeJsonAutoConfiguratorTest : CodeLensTestBase() {

    private lateinit var tempDir: Path
    private lateinit var claudeJsonPath: Path

    override fun setUp() {
        super.setUp()
        tempDir = Path.of(myFixture.tempDirPath, "claude-home")
        Files.createDirectories(tempDir)
        claudeJsonPath = tempDir.resolve(".claude.json")
        // Clean up any file left by a previous test method (same tempDir per test class)
        Files.deleteIfExists(claudeJsonPath)
        // Inject test path so the configurator doesn't touch the real ~/.claude.json
        ClaudeJsonAutoConfigurator.overridePath = claudeJsonPath
    }

    override fun tearDown() {
        try {
            ClaudeJsonAutoConfigurator.overridePath = null
        } finally {
            super.tearDown()
        }
    }

    // Test 1: file does not exist → create it with codelens entry
    fun testCreatesFileWhenMissing() {
        assertFalse(Files.exists(claudeJsonPath))

        ClaudeJsonAutoConfigurator.configure(24234)

        assertTrue(Files.exists(claudeJsonPath))
        val content = Files.readString(claudeJsonPath)
        assertTrue(content.contains("\"codelens\""))
        assertTrue(content.contains("http://127.0.0.1:24234/mcp"))
        assertTrue(content.contains("\"type\": \"http\""))
    }

    // Test 2: file exists but has no mcpServers key → adds entry
    fun testAddsEntryWhenMcpServersMissing() {
        Files.writeString(claudeJsonPath, """{"someOtherKey": "value"}""")

        ClaudeJsonAutoConfigurator.configure(24234)

        val content = Files.readString(claudeJsonPath)
        assertTrue(content.contains("\"mcpServers\""))
        assertTrue(content.contains("\"codelens\""))
        assertTrue(content.contains("http://127.0.0.1:24234/mcp"))
        // Preserves existing keys
        assertTrue(content.contains("\"someOtherKey\""))
    }

    // Test 3: file exists, codelens entry has same port → no-op (file unchanged)
    fun testNoOpWhenPortUnchanged() {
        val existing = """{
  "mcpServers": {
    "codelens": {
      "type": "http",
      "url": "http://127.0.0.1:24234/mcp"
    }
  }
}"""
        Files.writeString(claudeJsonPath, existing)
        val modifiedBefore = Files.getLastModifiedTime(claudeJsonPath)

        // Small sleep to ensure mtime would differ if written
        Thread.sleep(50)
        ClaudeJsonAutoConfigurator.configure(24234)

        val modifiedAfter = Files.getLastModifiedTime(claudeJsonPath)
        assertEquals(modifiedBefore, modifiedAfter)
    }

    // Test 4: file exists, codelens entry has different port → updates URL
    fun testUpdatesUrlWhenPortChanged() {
        Files.writeString(claudeJsonPath, """{
  "mcpServers": {
    "codelens": {
      "type": "http",
      "url": "http://127.0.0.1:9999/mcp"
    }
  }
}""")

        ClaudeJsonAutoConfigurator.configure(24234)

        val content = Files.readString(claudeJsonPath)
        assertTrue(content.contains("http://127.0.0.1:24234/mcp"))
        assertFalse(content.contains("9999"))
    }

    // Test 5: file exists with other mcpServers entries → preserves them
    fun testPreservesOtherMcpServerEntries() {
        Files.writeString(claudeJsonPath, """{
  "mcpServers": {
    "other-tool": {
      "type": "http",
      "url": "http://127.0.0.1:8080/mcp"
    }
  }
}""")

        ClaudeJsonAutoConfigurator.configure(24234)

        val content = Files.readString(claudeJsonPath)
        assertTrue(content.contains("\"other-tool\""))
        assertTrue(content.contains("http://127.0.0.1:8080/mcp"))
        assertTrue(content.contains("\"codelens\""))
        assertTrue(content.contains("http://127.0.0.1:24234/mcp"))
    }

    // Test 6: broken JSON → logs warning, does not crash, file unchanged
    fun testBrokenJsonDoesNotCrash() {
        val broken = "{ this is not valid json !!!"
        Files.writeString(claudeJsonPath, broken)

        // Must not throw
        ClaudeJsonAutoConfigurator.configure(24234)

        // File content should be overwritten with a new valid config (since broken JSON
        // falls back to empty object and needsUpdate returns true)
        val content = Files.readString(claudeJsonPath)
        assertTrue(content.contains("\"codelens\""))
    }
}
