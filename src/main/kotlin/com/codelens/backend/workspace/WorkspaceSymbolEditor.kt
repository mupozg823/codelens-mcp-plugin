package com.codelens.backend.workspace

import com.codelens.model.ModificationResult
import com.codelens.model.ReferenceInfo
import com.codelens.services.RenameScope
import java.nio.file.Files
import java.nio.file.Path
import kotlin.io.path.invariantSeparatorsPathString
import kotlin.io.path.readLines

internal class WorkspaceSymbolEditor(
    private val projectRoot: Path,
    private val relativize: (Path) -> String,
    private val displayPath: (Path) -> String,
    private val collectCandidateFiles: (Path) -> List<Path>,
    private val parseFileSymbols: (Path, Boolean) -> List<ParsedSymbol>,
    private val findReferences: (String, Path?, Int) -> List<ReferenceInfo>
) {

    fun renameSymbol(
        symbolName: String,
        targetFile: Path,
        newName: String,
        scope: RenameScope
    ): ModificationResult {
        require(IDENTIFIER_REGEX.matches(newName)) {
            "Invalid target symbol name: $newName"
        }

        val resolvedTarget = targetFile.normalize()
        val declaredSymbols = parseFileSymbols(resolvedTarget, false).flatMap(ParsedSymbol::flatten)
        val targetSymbol = resolveTargetSymbol(declaredSymbols, symbolName)
        if (targetSymbol == null) {
            return ModificationResult(false, "Symbol '$symbolName' not found in ${displayPath(targetFile)}")
        }

        val renamePattern = Regex("""\b${Regex.escape(targetSymbol.name)}\b""")
        if (scope == RenameScope.FILE) {
            val originalLines = runCatching { resolvedTarget.readLines() }.getOrElse {
                return ModificationResult(false, "Failed to read file: ${displayPath(targetFile)}")
            }
            val targetLines = originalLines.subList(targetSymbol.startLine - 1, targetSymbol.endLine)
            val replacementCount = targetLines.sumOf { renamePattern.findAll(it).count() }
            if (replacementCount == 0) {
                return ModificationResult(false, "Symbol '$symbolName' not found in ${displayPath(targetFile)}")
            }

            val renamedLines = targetLines.map { renamePattern.replace(it, newName) }
            val updatedLines = buildList {
                addAll(originalLines.subList(0, targetSymbol.startLine - 1))
                addAll(renamedLines)
                addAll(originalLines.subList(targetSymbol.endLine, originalLines.size))
            }
            Files.writeString(resolvedTarget, updatedLines.joinToString("\n"))

            return ModificationResult(
                success = true,
                message = "Renamed '$symbolName' to '$newName' in 1 file(s)",
                filePath = relativize(resolvedTarget),
                newContent = Files.readString(resolvedTarget)
            )
        }

        val searchRoots = collectCandidateFiles(projectRoot)
        val referencesByFile = findReferences(symbolName, resolvedTarget, Int.MAX_VALUE)
            .groupBy { projectRoot.resolve(it.filePath).normalize() }
        var modifiedFiles = 0
        var replacementCount = 0
        for (path in searchRoots) {
            val originalLines = runCatching { path.readLines() }.getOrNull() ?: continue
            val declaredInFile = parseFileSymbols(path, false).flatMap(ParsedSymbol::flatten)
            val sameNameDeclarations = declaredInFile.filter { it.name == targetSymbol.name }
            val updatedLines = originalLines.toMutableList()
            var matches = 0

            if (path == resolvedTarget) {
                val targetLines = originalLines.subList(targetSymbol.startLine - 1, targetSymbol.endLine)
                val declarationMatches = targetLines.sumOf { renamePattern.findAll(it).count() }
                if (declarationMatches > 0) {
                    val renamedLines = targetLines.map { renamePattern.replace(it, newName) }
                    for ((offset, line) in renamedLines.withIndex()) {
                        updatedLines[targetSymbol.startLine - 1 + offset] = line
                    }
                    matches += declarationMatches
                }
            } else if (sameNameDeclarations.isNotEmpty()) {
                continue
            }

            val fileReferences = referencesByFile[path].orEmpty()
                .filterNot { path == resolvedTarget && it.line in targetSymbol.startLine..targetSymbol.endLine }
            if (fileReferences.isNotEmpty()) {
                fileReferences
                    .sortedWith(compareByDescending<ReferenceInfo> { it.line }.thenByDescending { it.column })
                    .forEach { reference ->
                        val lineIndex = reference.line - 1
                        val updatedLine = replaceOccurrenceAtColumn(updatedLines[lineIndex], reference.column, targetSymbol.name, newName)
                        if (updatedLine != null) {
                            updatedLines[lineIndex] = updatedLine
                            matches += 1
                        }
                    }
            }
            if (matches == 0) continue

            Files.writeString(path, updatedLines.joinToString("\n"))
            modifiedFiles += 1
            replacementCount += matches
        }

        return ModificationResult(
            success = true,
            message = "Renamed '$symbolName' to '$newName' in $modifiedFiles file(s)",
            filePath = relativize(resolvedTarget),
            newContent = if (scope == RenameScope.FILE) Files.readString(resolvedTarget) else null
        )
    }

    fun replaceSymbolBody(
        symbolName: String,
        targetFile: Path,
        newBody: String
    ): ModificationResult {
        val resolvedTarget = targetFile.normalize()
        val declaredSymbol = resolveTargetSymbol(
            parseFileSymbols(resolvedTarget, false).flatMap(ParsedSymbol::flatten),
            symbolName
        )
            ?: return ModificationResult(false, "Symbol '$symbolName' not found in ${displayPath(targetFile)}")

        val originalLines = runCatching { resolvedTarget.readLines() }.getOrElse {
            return ModificationResult(false, "Failed to read file: ${displayPath(targetFile)}")
        }
        val replacementLines = newBody.lines()
        val updatedLines = buildList {
            addAll(originalLines.subList(0, declaredSymbol.startLine - 1))
            addAll(replacementLines)
            addAll(originalLines.subList(declaredSymbol.endLine, originalLines.size))
        }
        Files.writeString(resolvedTarget, updatedLines.joinToString("\n"))

        return ModificationResult(
            success = true,
            message = "Replaced body of '$symbolName' in ${relativize(resolvedTarget)}",
            filePath = relativize(resolvedTarget),
            affectedLines = declaredSymbol.startLine..(declaredSymbol.startLine + replacementLines.size - 1),
            newContent = Files.readString(resolvedTarget)
        )
    }

    fun insertAfterSymbol(
        symbolName: String,
        targetFile: Path,
        content: String
    ): ModificationResult {
        val resolvedTarget = targetFile.normalize()
        val declaredSymbol = resolveTargetSymbol(
            parseFileSymbols(resolvedTarget, false).flatMap(ParsedSymbol::flatten),
            symbolName
        )
            ?: return ModificationResult(false, "Symbol '$symbolName' not found in ${displayPath(targetFile)}")

        val originalLines = runCatching { resolvedTarget.readLines() }.getOrElse {
            return ModificationResult(false, "Failed to read file: ${displayPath(targetFile)}")
        }
        val insertedLines = content.lines()
        val updatedLines = buildList {
            addAll(originalLines.subList(0, declaredSymbol.endLine))
            addAll(insertedLines)
            addAll(originalLines.subList(declaredSymbol.endLine, originalLines.size))
        }
        Files.writeString(resolvedTarget, updatedLines.joinToString("\n"))

        return ModificationResult(
            success = true,
            message = "Inserted content after '$symbolName' in ${relativize(resolvedTarget)}",
            filePath = relativize(resolvedTarget),
            affectedLines = (declaredSymbol.endLine + 1)..(declaredSymbol.endLine + insertedLines.size),
            newContent = Files.readString(resolvedTarget)
        )
    }

    fun insertBeforeSymbol(
        symbolName: String,
        targetFile: Path,
        content: String
    ): ModificationResult {
        val resolvedTarget = targetFile.normalize()
        val declaredSymbol = resolveTargetSymbol(
            parseFileSymbols(resolvedTarget, false).flatMap(ParsedSymbol::flatten),
            symbolName
        )
            ?: return ModificationResult(false, "Symbol '$symbolName' not found in ${displayPath(targetFile)}")

        val originalLines = runCatching { resolvedTarget.readLines() }.getOrElse {
            return ModificationResult(false, "Failed to read file: ${displayPath(targetFile)}")
        }
        val insertedLines = content.lines()
        val updatedLines = buildList {
            addAll(originalLines.subList(0, declaredSymbol.startLine - 1))
            addAll(insertedLines)
            addAll(originalLines.subList(declaredSymbol.startLine - 1, originalLines.size))
        }
        Files.writeString(resolvedTarget, updatedLines.joinToString("\n"))

        return ModificationResult(
            success = true,
            message = "Inserted content before '$symbolName' in ${relativize(resolvedTarget)}",
            filePath = relativize(resolvedTarget),
            affectedLines = declaredSymbol.startLine..(declaredSymbol.startLine + insertedLines.size - 1),
            newContent = Files.readString(resolvedTarget)
        )
    }

    fun isWriteReference(line: String, symbolName: String): Boolean {
        val assignmentRegex = Regex("""\b${Regex.escape(symbolName)}\b\s*([+\-*/%]?=)""")
        return assignmentRegex.containsMatchIn(line)
    }

    fun isDeclarationLine(line: String, symbolName: String, dummyPath: Path): Boolean {
        return parseDeclaration(line, dummyPath, 0, listOf(line), false, relativize)?.symbol?.name == symbolName
    }

    fun isCodeOccurrence(line: String, matchStart: Int): Boolean {
        val trimmed = line.trimStart()
        if (
            trimmed.startsWith("//") ||
            trimmed.startsWith("#") ||
            trimmed.startsWith("*") ||
            trimmed.startsWith("/*")
        ) {
            return false
        }

        val prefix = line.substring(0, matchStart)
        val lastLineComment = prefix.lastIndexOf("//")
        if (lastLineComment >= 0) {
            return false
        }

        val doubleQuotes = prefix.count { it == '"' && (prefix.indexOf(it) >= 0) }
        val singleQuotes = prefix.count { it == '\'' && (prefix.indexOf(it) >= 0) }
        return doubleQuotes % 2 == 0 && singleQuotes % 2 == 0
    }

    fun replaceOccurrenceAtColumn(line: String, column: Int, oldName: String, newName: String): String? {
        val startIndex = column - 1
        val endIndex = startIndex + oldName.length
        if (startIndex < 0 || endIndex > line.length) return null
        if (line.substring(startIndex, endIndex) != oldName) return null
        return line.substring(0, startIndex) + newName + line.substring(endIndex)
    }

    fun resolveTargetSymbol(symbols: List<ParsedSymbol>, selector: String): ParsedSymbol? {
        return if (isNamePathSelector(selector)) {
            symbols.firstOrNull { it.namePath == selector.removePrefix("/") }
        } else {
            symbols.firstOrNull { it.name == selector }
        }
    }

    fun resolveReferenceScope(symbols: List<ParsedSymbol>, targetSymbol: ParsedSymbol): IntRange {
        val ownerPath = targetSymbol.namePath.substringBeforeLast("/", "")
        val owner = ownerPath.takeIf { it.isNotEmpty() }?.let { path ->
            symbols.firstOrNull { it.namePath == path }
        }
        val scopeSymbol = owner ?: targetSymbol
        return scopeSymbol.startLine..scopeSymbol.endLine
    }

    private fun isNamePathSelector(selector: String): Boolean = selector.contains("/")
}
