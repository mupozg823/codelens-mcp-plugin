package com.codelens.plugin

import com.intellij.openapi.diagnostic.Logger
import java.nio.file.Files
import java.nio.file.Path

/**
 * Installs a companion skill file to ~/.claude/skills/ on plugin startup.
 * The skill guides Claude Code on how to effectively use CodeLens MCP tools.
 */
object CompanionSkillInstaller {

    private val logger = Logger.getInstance(CompanionSkillInstaller::class.java)

    private const val SKILL_FILENAME = "codelens-mcp.md"
    private const val SKILL_VERSION = "1.0.0"

    fun install() {
        try {
            val skillsDir = Path.of(System.getProperty("user.home"), ".claude", "skills")
            if (!Files.exists(skillsDir)) {
                Files.createDirectories(skillsDir)
            }

            val skillFile = skillsDir.resolve(SKILL_FILENAME)

            // Check if already installed with current version
            if (Files.exists(skillFile)) {
                val content = Files.readString(skillFile)
                if (content.contains("version: $SKILL_VERSION")) {
                    logger.info("CodeLens companion skill already up to date (v$SKILL_VERSION)")
                    return
                }
            }

            Files.writeString(skillFile, SKILL_CONTENT)
            logger.info("CodeLens companion skill installed to $skillFile")
        } catch (e: Exception) {
            logger.warn("Failed to install companion skill: ${e.message}")
        }
    }

    private val SKILL_CONTENT = """
---
name: codelens-mcp
description: Guide for using CodeLens MCP tools effectively with Claude Code
version: $SKILL_VERSION
---

# CodeLens MCP v1.0 — Agent Guide

CodeLens provides 64+ symbol-level code intelligence tools via MCP.
Three backends: JetBrains PSI (IntelliJ), Tree-sitter AST (Standalone), Workspace regex (fallback).

## Core Principle: Symbols First, Files Never

Do NOT read entire files. Use symbol tools instead:

1. **Overview** → `get_symbols_overview` (structure without reading file content)
2. **Find symbol** → `find_symbol` with `include_body=true` (only the code you need)
3. **By stable ID** → `find_symbol` with `symbol_id` (fastest, e.g. `src/main.py#function:main`)
4. **Token-efficient** → `get_ranked_context` with `max_tokens` budget (auto-ranks relevant symbols)

## Symbol Editing (Precise, No Line Counting)

- `replace_symbol_body` — replace function/class body by name
- `insert_after_symbol` / `insert_before_symbol` — add code adjacent to a symbol
- `rename_symbol` — rename across project with refactoring support

## Code Intelligence

- `find_referencing_symbols` — who uses this symbol?
- `get_call_hierarchy` — callers/callees (PSI only)
- `get_type_hierarchy` — supertypes/subtypes
- `find_importers` — reverse import dependency ("who imports this file?")
- `get_blast_radius` — transitive change impact with depth scores
- `get_symbol_importance` — PageRank file ranking on import graph

## Analysis

- `get_complexity` — cyclomatic complexity per function
- `find_tests` — auto-detect test functions across project
- `find_annotations` — collect TODO/FIXME/HACK/DEPRECATED comments
- `find_dead_code` — symbols with zero importers

## Git Integration

- `get_diff_symbols` — map git diff to affected symbols
- `get_changed_files` — changed files with symbol counts

## Best Practices

1. **Start with overview**: `get_symbols_overview` before diving into specific symbols
2. **Use stable IDs**: Symbols have IDs like `file#kind:namePath` — use `symbol_id` for precise lookup
3. **Token budget**: Use `get_ranked_context` with `max_tokens` to stay within limits
4. **Before editing**: Check `find_referencing_symbols` and `get_blast_radius`
5. **Prefer symbol ops**: `replace_symbol_body` over line-based edits
6. **Check impact**: `get_diff_symbols` after changes to verify what was affected

## Supported Languages

Tree-sitter (Standalone): Python, JS/TS/TSX, Go, Rust, Ruby, Java, Kotlin, C, C++, PHP, Swift, Scala (14 languages)
PSI (IntelliJ): Java, Kotlin, JS/TS, Groovy, Shell, Python (6 languages, deeper analysis)
""".trimIndent()
}
