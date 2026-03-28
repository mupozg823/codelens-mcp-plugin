package com.codelens.backend.workspace

import java.nio.file.Path
import kotlin.io.path.extension
import kotlin.io.path.readLines

internal enum class ScopeStrategy { BRACE, INDENT, END_KEYWORD }

internal data class Scope(val symbol: ParsedSymbol, val targetDepth: Int)
internal data class IndentScope(val symbol: ParsedSymbol, val indentLevel: Int)
internal data class ParsedDeclaration(val symbol: ParsedSymbol, val opensScope: Boolean)

internal fun scopeStrategyFor(path: Path): ScopeStrategy = when (path.extension) {
    "py" -> ScopeStrategy.INDENT
    "rb" -> ScopeStrategy.END_KEYWORD
    else -> ScopeStrategy.BRACE
}

internal fun parseFile(
    path: Path,
    includeBodies: Boolean,
    relativize: (Path) -> String
): List<ParsedSymbol> {
    val lines = runCatching { path.readLines() }.getOrNull() ?: return emptyList()
    return when (scopeStrategyFor(path)) {
        ScopeStrategy.INDENT -> parseFileIndentScoped(lines, path, includeBodies, relativize)
        ScopeStrategy.END_KEYWORD -> parseFileEndKeywordScoped(lines, path, includeBodies, relativize)
        ScopeStrategy.BRACE -> parseFileBraceScoped(lines, path, includeBodies, relativize)
    }
}

/** Brace-based scope tracking (Java, Kotlin, JS/TS, Go, Rust, Swift, C/C++, etc.) */
internal fun parseFileBraceScoped(
    lines: List<String>,
    path: Path,
    includeBodies: Boolean,
    relativize: (Path) -> String
): List<ParsedSymbol> {
    val roots = mutableListOf<ParsedSymbol>()
    val scopes = ArrayDeque<Scope>()
    var braceDepth = 0

    for ((index, line) in lines.withIndex()) {
        val declaration = parseDeclaration(line, path, index, lines, includeBodies, relativize)
        if (declaration != null) {
            val symbol = declaration.symbol
            val parent = scopes.lastOrNull()?.symbol
            symbol.namePath = if (parent != null) "${parent.namePath}/${symbol.name}" else symbol.name
            if (parent != null) {
                parent.children.add(symbol)
            } else {
                roots.add(symbol)
            }
            if (declaration.opensScope) {
                scopes.addLast(Scope(symbol, braceDepth + 1))
            }
        }

        braceDepth += line.count { it == '{' } - line.count { it == '}' }
        while (scopes.isNotEmpty() && scopes.last().targetDepth > braceDepth) {
            scopes.removeLast().symbol.endLine = index + 1
        }
    }

    while (scopes.isNotEmpty()) {
        scopes.removeLast().symbol.endLine = lines.size
    }

    return roots
}

/** Indentation-based scope tracking (Python) */
internal fun parseFileIndentScoped(
    lines: List<String>,
    path: Path,
    includeBodies: Boolean,
    relativize: (Path) -> String
): List<ParsedSymbol> {
    val roots = mutableListOf<ParsedSymbol>()
    val scopes = ArrayDeque<IndentScope>()

    for ((index, line) in lines.withIndex()) {
        val stripped = line.trimEnd()
        if (stripped.isBlank() || stripped.startsWith("#")) continue

        val indent = line.length - line.trimStart().length

        // Pop scopes whose indent level is >= current line's indent
        while (scopes.isNotEmpty() && scopes.last().indentLevel >= indent) {
            scopes.removeLast().symbol.endLine = findLastNonBlankLine(lines, index - 1) + 1
        }

        val declaration = parseDeclaration(line, path, index, lines, includeBodies, relativize)
        if (declaration != null) {
            val symbol = declaration.symbol
            val parent = scopes.lastOrNull()?.symbol
            symbol.namePath = if (parent != null) "${parent.namePath}/${symbol.name}" else symbol.name
            if (parent != null) {
                parent.children.add(symbol)
            } else {
                roots.add(symbol)
            }
            // Python class/def opens scope if line ends with ':'
            if (stripped.endsWith(":")) {
                scopes.addLast(IndentScope(symbol, indent))
            }
        }
    }

    while (scopes.isNotEmpty()) {
        scopes.removeLast().symbol.endLine = lines.size
    }

    return roots
}

/** End-keyword scope tracking (Ruby) */
internal fun parseFileEndKeywordScoped(
    lines: List<String>,
    path: Path,
    includeBodies: Boolean,
    relativize: (Path) -> String
): List<ParsedSymbol> {
    val roots = mutableListOf<ParsedSymbol>()
    val scopes = ArrayDeque<Scope>()
    var endKeywordDepth = 0

    for ((index, line) in lines.withIndex()) {
        val trimmed = line.trimStart()

        // Count scope openers (only at statement level, not in strings)
        if (RUBY_SCOPE_OPENER.containsMatchIn(trimmed)) {
            endKeywordDepth++
        }

        val declaration = parseDeclaration(line, path, index, lines, includeBodies, relativize)
        if (declaration != null) {
            val symbol = declaration.symbol
            val parent = scopes.lastOrNull()?.symbol
            symbol.namePath = if (parent != null) "${parent.namePath}/${symbol.name}" else symbol.name
            if (parent != null) {
                parent.children.add(symbol)
            } else {
                roots.add(symbol)
            }
            // Ruby class/module/def always open a scope
            if (trimmed.startsWith("class ") || trimmed.startsWith("module ") || trimmed.startsWith("def ")) {
                scopes.addLast(Scope(symbol, endKeywordDepth))
            }
        }

        // Count 'end' keywords
        if (trimmed == "end" || trimmed.startsWith("end ") || trimmed.startsWith("end;")) {
            endKeywordDepth--
            while (scopes.isNotEmpty() && scopes.last().targetDepth > endKeywordDepth) {
                scopes.removeLast().symbol.endLine = index + 1
            }
        }
    }

    while (scopes.isNotEmpty()) {
        scopes.removeLast().symbol.endLine = lines.size
    }

    return roots
}

internal fun findLastNonBlankLine(lines: List<String>, fromIndex: Int): Int {
    for (i in fromIndex downTo 0) {
        if (lines[i].isNotBlank()) return i
    }
    return fromIndex
}
