package com.codelens.backend.workspace

import com.codelens.model.ReferenceInfo
import com.codelens.model.ModificationResult
import com.codelens.model.SymbolInfo
import com.codelens.model.SymbolKind
import com.codelens.services.RenameScope
import com.codelens.tools.SharedContract
import java.nio.file.Files
import java.nio.file.Path
import java.util.regex.Pattern
import kotlin.io.path.extension
import kotlin.io.path.invariantSeparatorsPathString
import kotlin.io.path.isDirectory
import kotlin.io.path.name
import kotlin.io.path.readLines

internal class WorkspaceSymbolScanner(private val projectRoot: Path) {

    fun getSymbolsOverview(path: Path, depth: Int): List<SymbolInfo> {
        return collectCandidateFiles(path)
            .flatMap { parseFile(it, includeBodies = false).map { symbol -> symbol.toSymbolInfo(depth) } }
    }

    fun findSymbols(
        name: String,
        filePath: Path?,
        includeBody: Boolean,
        exactMatch: Boolean
    ): List<SymbolInfo> {
        val matcher: (ParsedSymbol) -> Boolean = if (isNamePathSelector(name)) {
            { candidate -> matchesNamePathPattern(name, candidate.namePath) }
        } else if (exactMatch) {
            { candidate -> candidate.name == name }
        } else {
            { candidate -> candidate.name.contains(name, ignoreCase = true) }
        }

        return collectCandidateFiles(filePath ?: projectRoot)
            .flatMap { parseFile(it, includeBody).flatMap(ParsedSymbol::flatten) }
            .filter(matcher)
            .map { it.toSymbolInfo(Int.MAX_VALUE) }
    }

    fun findReferences(symbolName: String, definitionFile: Path?, maxResults: Int): List<ReferenceInfo> {
        val resolvedDefinitionFile = definitionFile?.normalize()
        val definitionSymbols = resolvedDefinitionFile
            ?.let { parseFile(it, includeBodies = false).flatMap(ParsedSymbol::flatten) }
            .orEmpty()
        val targetSymbol = if (resolvedDefinitionFile != null) {
            resolveTargetSymbol(definitionSymbols, symbolName) ?: return emptyList()
        } else {
            null
        }
        val referenceName = targetSymbol?.name ?: symbolName.removePrefix("/").substringAfterLast("/")
        val pattern = Pattern.compile("\\b${Pattern.quote(referenceName)}\\b")
        val results = mutableListOf<ReferenceInfo>()

        for (file in collectCandidateFiles(projectRoot)) {
            if (results.size >= maxResults) break

            val lines = runCatching { file.readLines() }.getOrNull() ?: continue
            val symbols = parseFile(file, includeBodies = false).flatMap(ParsedSymbol::flatten)
            val sameNameDeclarations = symbols.filter { it.name == referenceName }
            val sameFileReferenceScope = when {
                resolvedDefinitionFile == null || file != resolvedDefinitionFile -> null
                sameNameDeclarations.size <= 1 -> null
                else -> resolveReferenceScope(definitionSymbols, targetSymbol!!)
            }

            if (resolvedDefinitionFile != null && file != resolvedDefinitionFile && sameNameDeclarations.isNotEmpty()) {
                continue
            }

            for ((index, line) in lines.withIndex()) {
                if (results.size >= maxResults) break
                val lineNumber = index + 1
                if (sameFileReferenceScope != null && lineNumber !in sameFileReferenceScope) continue
                if (isDeclarationLine(line, referenceName)) continue

                val matcher = pattern.matcher(line)
                if (!matcher.find()) continue
                if (!isCodeOccurrence(line, matcher.start())) continue

                val container = symbols
                    .filter { it.startLine <= lineNumber && lineNumber <= it.endLine }
                    .minByOrNull { it.endLine - it.startLine }

                results.add(
                    ReferenceInfo(
                        filePath = relativize(file),
                        line = lineNumber,
                        column = matcher.start() + 1,
                        containingSymbol = container?.name ?: file.name,
                        context = line.trim(),
                        isWrite = isWriteReference(line, referenceName)
                    )
                )
            }
        }

        return results
    }

    fun getTypeHierarchy(fullyQualifiedName: String): Map<String, Any?> {
        val declarations = collectCandidateFiles(projectRoot).mapNotNull { parseTypeDeclaration(it) }
        val target = declarations.firstOrNull { it.qualifiedName == fullyQualifiedName }
            ?: declarations.firstOrNull { it.name == fullyQualifiedName.substringAfterLast('.') }
            ?: return mapOf(
                "error" to "Class not found: $fullyQualifiedName",
                "backend" to "Workspace",
                "fully_qualified_name" to fullyQualifiedName
            )

        val subtypes = declarations.filter { declaration ->
            declaration.supertypes.any { supertype ->
                supertype == target.name || supertype == target.qualifiedName
            }
        }

        return mapOf(
            "class_name" to target.name,
            "fully_qualified_name" to target.qualifiedName,
            "kind" to target.kind,
            "supertypes" to target.supertypes.map { supertype ->
                mapOf(
                    "name" to supertype.substringAfterLast('.'),
                    "qualified_name" to declarations.firstOrNull { it.name == supertype || it.qualifiedName == supertype }?.qualifiedName.orEmpty(),
                    "kind" to declarations.firstOrNull { it.name == supertype || it.qualifiedName == supertype }?.kind.orEmpty()
                )
            },
            "subtypes" to subtypes.map { subtype ->
                mapOf(
                    "name" to subtype.name,
                    "qualified_name" to subtype.qualifiedName
                )
            },
            "members" to mapOf(
                "methods" to emptyList<String>(),
                "fields" to emptyList<String>(),
                "properties" to target.properties
            ),
            "type_parameters" to emptyList<Map<String, String>>(),
            "backend" to "Workspace"
        )
    }

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
        val declaredSymbols = parseFile(resolvedTarget, includeBodies = false).flatMap(ParsedSymbol::flatten)
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
            val declaredInFile = parseFile(path, includeBodies = false).flatMap(ParsedSymbol::flatten)
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
            parseFile(resolvedTarget, includeBodies = false).flatMap(ParsedSymbol::flatten),
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
            parseFile(resolvedTarget, includeBodies = false).flatMap(ParsedSymbol::flatten),
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
            parseFile(resolvedTarget, includeBodies = false).flatMap(ParsedSymbol::flatten),
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

    private fun collectCandidateFiles(path: Path): List<Path> {
        val resolved = path.normalize()
        require(Files.exists(resolved)) { "Path not found: ${displayPath(path)}" }

        return if (resolved.isDirectory()) {
            Files.walk(resolved).use { walk ->
                walk.filter { Files.isRegularFile(it) }
                    .filter(::isSearchableSourceFile)
                    .sorted()
                    .toList()
            }
        } else {
            require(isSearchableSourceFile(resolved)) { "Unsupported source file: ${displayPath(path)}" }
            listOf(resolved)
        }
    }

    private fun parseFile(path: Path, includeBodies: Boolean): List<ParsedSymbol> {
        val lines = runCatching { path.readLines() }.getOrNull() ?: return emptyList()
        return when (scopeStrategyFor(path)) {
            ScopeStrategy.INDENT -> parseFileIndentScoped(lines, path, includeBodies)
            ScopeStrategy.END_KEYWORD -> parseFileEndKeywordScoped(lines, path, includeBodies)
            ScopeStrategy.BRACE -> parseFileBraceScoped(lines, path, includeBodies)
        }
    }

    /** Brace-based scope tracking (Java, Kotlin, JS/TS, Go, Rust, Swift, C/C++, etc.) */
    private fun parseFileBraceScoped(lines: List<String>, path: Path, includeBodies: Boolean): List<ParsedSymbol> {
        val roots = mutableListOf<ParsedSymbol>()
        val scopes = ArrayDeque<Scope>()
        var braceDepth = 0

        for ((index, line) in lines.withIndex()) {
            val declaration = parseDeclaration(line, path, index, lines, includeBodies)
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
    private fun parseFileIndentScoped(lines: List<String>, path: Path, includeBodies: Boolean): List<ParsedSymbol> {
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

            val declaration = parseDeclaration(line, path, index, lines, includeBodies)
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
    private fun parseFileEndKeywordScoped(lines: List<String>, path: Path, includeBodies: Boolean): List<ParsedSymbol> {
        val roots = mutableListOf<ParsedSymbol>()
        val scopes = ArrayDeque<Scope>()
        var endKeywordDepth = 0

        for ((index, line) in lines.withIndex()) {
            val trimmed = line.trimStart()

            // Count scope openers (only at statement level, not in strings)
            if (RUBY_SCOPE_OPENER.containsMatchIn(trimmed)) {
                endKeywordDepth++
            }

            val declaration = parseDeclaration(line, path, index, lines, includeBodies)
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

    private fun findLastNonBlankLine(lines: List<String>, fromIndex: Int): Int {
        for (i in fromIndex downTo 0) {
            if (lines[i].isNotBlank()) return i
        }
        return fromIndex
    }

    private fun parseDeclaration(
        line: String,
        path: Path,
        index: Int,
        lines: List<String>,
        includeBodies: Boolean
    ): ParsedDeclaration? {
        val trimmed = line.trimStart()
        if (STATEMENT_PREFIXES.any { trimmed.startsWith(it) }) {
            return null
        }

        val classMatch = CLASS_REGEXES.firstNotNullOfOrNull { it.find(line) }
        if (classMatch != null) {
            val name = classMatch.groups[2]?.value ?: return null
            val kind = classKindFor(classMatch.groups[1]?.value.orEmpty())
            val symbol = ParsedSymbol(
                name = name,
                kind = kind,
                filePath = relativize(path),
                line = index + 1,
                column = classMatch.range.first + 1,
                signature = line.trim(),
                startLine = index + 1,
                endLine = index + 1,
                body = if (includeBodies) extractBody(lines, index) else null
            )
            return ParsedDeclaration(symbol, opensScope = line.contains('{'))
        }

        // Go: type Name struct/interface (reversed group order: group1=name, group2=kind)
        val goTypeMatch = GO_TYPE_REGEX.find(line)
        if (goTypeMatch != null) {
            val name = goTypeMatch.groups[1]?.value ?: return null
            val kind = classKindFor(goTypeMatch.groups[2]?.value.orEmpty())
            val symbol = ParsedSymbol(
                name = name,
                kind = kind,
                filePath = relativize(path),
                line = index + 1,
                column = goTypeMatch.range.first + 1,
                signature = line.trim(),
                startLine = index + 1,
                endLine = index + 1,
                body = if (includeBodies) extractBody(lines, index) else null
            )
            return ParsedDeclaration(symbol, opensScope = line.contains('{'))
        }

        val functionMatch = FUNCTION_REGEXES.firstNotNullOfOrNull { it.find(line) }
        if (functionMatch != null) {
            val name = functionMatch.groups[1]?.value ?: return null
            if (name in RESERVED_WORDS) return null
            val symbol = ParsedSymbol(
                name = name,
                kind = SymbolKind.FUNCTION,
                filePath = relativize(path),
                line = index + 1,
                column = functionMatch.range.first + 1,
                signature = line.trim(),
                startLine = index + 1,
                endLine = index + 1,
                body = if (includeBodies) extractBody(lines, index) else null
            )
            return ParsedDeclaration(symbol, opensScope = line.contains('{'))
        }

        val propertyMatch = PROPERTY_REGEXES.firstNotNullOfOrNull { it.find(line) }
        if (propertyMatch != null) {
            val name = propertyMatch.groups[2]?.value ?: return null
            val symbol = ParsedSymbol(
                name = name,
                kind = SymbolKind.PROPERTY,
                filePath = relativize(path),
                line = index + 1,
                column = propertyMatch.range.first + 1,
                signature = line.trim(),
                startLine = index + 1,
                endLine = index + 1,
                body = if (includeBodies) line.trim() else null
            )
            return ParsedDeclaration(symbol, opensScope = false)
        }

        return null
    }

    private fun extractBody(lines: List<String>, startIndex: Int): String {
        val startLine = lines[startIndex]

        // Python/Ruby: indent or end-keyword based body
        if (startLine.trimEnd().endsWith(":")) {
            return extractIndentBody(lines, startIndex)
        }

        if (!startLine.contains('{')) {
            // Ruby: def method_name / class Name without braces
            if (startLine.trimStart().let { it.startsWith("def ") || it.startsWith("class ") || it.startsWith("module ") }) {
                return extractEndKeywordBody(lines, startIndex)
            }
            return startLine.trim()
        }

        var depth = 0
        for (lineIndex in startIndex until lines.size) {
            val line = lines[lineIndex]
            depth += line.count { it == '{' }
            depth -= line.count { it == '}' }
            if (depth <= 0 && lineIndex > startIndex) {
                return lines.subList(startIndex, lineIndex + 1).joinToString("\n")
            }
        }

        return lines.subList(startIndex, lines.size).joinToString("\n")
    }

    /** Extract body for Python (indentation-based) */
    private fun extractIndentBody(lines: List<String>, startIndex: Int): String {
        val baseIndent = lines[startIndex].length - lines[startIndex].trimStart().length
        for (lineIndex in (startIndex + 1) until lines.size) {
            val line = lines[lineIndex]
            if (line.isBlank()) continue
            val indent = line.length - line.trimStart().length
            if (indent <= baseIndent) {
                return lines.subList(startIndex, lineIndex).joinToString("\n")
            }
        }
        return lines.subList(startIndex, lines.size).joinToString("\n")
    }

    /** Extract body for Ruby (end-keyword based) */
    private fun extractEndKeywordBody(lines: List<String>, startIndex: Int): String {
        var depth = 1
        for (lineIndex in (startIndex + 1) until lines.size) {
            val trimmed = lines[lineIndex].trimStart()
            if (RUBY_SCOPE_OPENER.containsMatchIn(trimmed)) depth++
            if (trimmed == "end" || trimmed.startsWith("end ") || trimmed.startsWith("end;")) {
                depth--
                if (depth <= 0) {
                    return lines.subList(startIndex, lineIndex + 1).joinToString("\n")
                }
            }
        }
        return lines.subList(startIndex, lines.size).joinToString("\n")
    }

    private fun parseTypeDeclaration(path: Path): ParsedTypeDeclaration? {
        val lines = runCatching { path.readLines() }.getOrNull() ?: return null
        val packageName = lines.firstNotNullOfOrNull { PACKAGE_REGEX.find(it)?.groupValues?.getOrNull(1) }.orEmpty()
        val declarationLine = lines.firstNotNullOfOrNull { line ->
            CLASS_REGEXES.firstNotNullOfOrNull { regex -> regex.find(line) }?.let { match -> line to match }
        } ?: return null

        val line = declarationLine.first
        val match = declarationLine.second
        val name = match.groups[2]?.value ?: return null
        val kind = classKindForDeclaration(line, match.groups[1]?.value.orEmpty())
        val qualifiedName = if (packageName.isNotEmpty()) "$packageName.$name" else name
        return ParsedTypeDeclaration(
            name = name,
            qualifiedName = qualifiedName,
            kind = kind,
            supertypes = extractSupertypes(line),
            properties = extractPrimaryProperties(line)
        )
    }

    private fun classKindForDeclaration(line: String, token: String): String = when {
        line.contains("data class") -> "data_class"
        classKindFor(token) == SymbolKind.INTERFACE -> "interface"
        classKindFor(token) == SymbolKind.ENUM -> "enum"
        classKindFor(token) == SymbolKind.OBJECT -> "object"
        else -> "class"
    }

    private fun extractSupertypes(line: String): List<String> {
        val javaMatches = buildList {
            EXTENDS_REGEX.find(line)?.groupValues?.getOrNull(1)?.let { add(normalizeTypeName(it)) }
            IMPLEMENTS_REGEX.find(line)?.groupValues?.getOrNull(1)
                ?.split(',')
                ?.map { it.trim() }
                ?.filter { it.isNotEmpty() }
                ?.mapTo(this) { normalizeTypeName(it) }
        }
        if (javaMatches.isNotEmpty()) {
            return javaMatches
        }

        val kotlinClause = line.substringAfter(':', "").substringBefore('{').trim()
        if (kotlinClause.isEmpty()) return emptyList()
        return kotlinClause.split(',')
            .map { normalizeTypeName(it) }
            .filter { it.isNotEmpty() }
    }

    private fun normalizeTypeName(raw: String): String {
        val trimmed = raw.trim().substringBefore(" where ").substringBefore("<")
        return trimmed.substringBefore("(").substringAfterLast('.')
    }

    private fun extractPrimaryProperties(line: String): List<String> {
        val parameterBlock = line.substringAfter("(", "").substringBeforeLast(")", "")
        if (parameterBlock.isEmpty()) return emptyList()
        return parameterBlock.split(',')
            .map { it.trim() }
            .mapNotNull { parameter ->
                PRIMARY_PROPERTY_REGEX.find(parameter)?.groupValues?.getOrNull(1)
            }
    }

    private fun classKindFor(token: String): SymbolKind = when (token.trim()) {
        "interface" -> SymbolKind.INTERFACE
        "enum", "enum class" -> SymbolKind.ENUM
        "object" -> SymbolKind.OBJECT
        "annotation class" -> SymbolKind.ANNOTATION
        "trait", "protocol" -> SymbolKind.INTERFACE
        "namespace", "module" -> SymbolKind.MODULE
        "struct", "union", "record" -> SymbolKind.CLASS
        else -> SymbolKind.CLASS
    }

    private fun isWriteReference(line: String, symbolName: String): Boolean {
        val assignmentRegex = Regex("""\b${Regex.escape(symbolName)}\b\s*([+\-*/%]?=)""")
        return assignmentRegex.containsMatchIn(line)
    }

    private fun isDeclarationLine(line: String, symbolName: String): Boolean {
        return parseDeclaration(line, projectRoot.resolve("_"), 0, listOf(line), includeBodies = false)?.symbol?.name == symbolName
    }

    private fun isCodeOccurrence(line: String, matchStart: Int): Boolean {
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

    private fun replaceOccurrenceAtColumn(line: String, column: Int, oldName: String, newName: String): String? {
        val startIndex = column - 1
        val endIndex = startIndex + oldName.length
        if (startIndex < 0 || endIndex > line.length) return null
        if (line.substring(startIndex, endIndex) != oldName) return null
        return line.substring(0, startIndex) + newName + line.substring(endIndex)
    }

    private fun resolveTargetSymbol(symbols: List<ParsedSymbol>, selector: String): ParsedSymbol? {
        return if (isNamePathSelector(selector)) {
            symbols.firstOrNull { it.namePath == selector.removePrefix("/") }
        } else {
            symbols.firstOrNull { it.name == selector }
        }
    }

    private fun resolveReferenceScope(symbols: List<ParsedSymbol>, targetSymbol: ParsedSymbol): IntRange {
        val ownerPath = targetSymbol.namePath.substringBeforeLast("/", "")
        val owner = ownerPath.takeIf { it.isNotEmpty() }?.let { path ->
            symbols.firstOrNull { it.namePath == path }
        }
        val scopeSymbol = owner ?: targetSymbol
        return scopeSymbol.startLine..scopeSymbol.endLine
    }

    private fun matchesNamePathPattern(pattern: String, namePath: String): Boolean {
        val normalizedPattern = pattern.removePrefix("/")
        return when {
            pattern.startsWith("/") -> namePath == normalizedPattern
            normalizedPattern.contains("/") -> namePath == normalizedPattern || namePath.endsWith("/$normalizedPattern")
            else -> namePath.substringAfterLast("/") == normalizedPattern
        }
    }

    private fun isNamePathSelector(selector: String): Boolean = selector.contains("/")

    private fun relativize(path: Path): String {
        return projectRoot.relativize(path.normalize()).invariantSeparatorsPathString
    }

    private fun displayPath(path: Path): String {
        return runCatching { relativize(path) }.getOrElse { path.invariantSeparatorsPathString }
    }

    private fun isSearchableSourceFile(path: Path): Boolean {
        if (!Files.isRegularFile(path)) return false
        return path.extension in SEARCHABLE_EXTENSIONS
    }

    private enum class ScopeStrategy { BRACE, INDENT, END_KEYWORD }

    private fun scopeStrategyFor(path: Path): ScopeStrategy = when (path.extension) {
        "py" -> ScopeStrategy.INDENT
        "rb" -> ScopeStrategy.END_KEYWORD
        else -> ScopeStrategy.BRACE
    }

    private data class Scope(val symbol: ParsedSymbol, val targetDepth: Int)
    private data class IndentScope(val symbol: ParsedSymbol, val indentLevel: Int)

    private data class ParsedDeclaration(val symbol: ParsedSymbol, val opensScope: Boolean)

    private data class ParsedSymbol(
        val name: String,
        val kind: SymbolKind,
        val filePath: String,
        val line: Int,
        val column: Int,
        val signature: String,
        val startLine: Int,
        var endLine: Int,
        var namePath: String = name,
        val body: String?,
        val children: MutableList<ParsedSymbol> = mutableListOf()
    ) {
        fun toSymbolInfo(depth: Int): SymbolInfo {
            return SymbolInfo(
                name = name,
                kind = kind,
                filePath = filePath,
                line = line,
                column = column,
                signature = signature,
                namePath = namePath,
                body = body,
                children = if (depth > 1) children.map { it.toSymbolInfo(depth - 1) } else emptyList()
            )
        }

        fun flatten(): List<ParsedSymbol> {
            return buildList {
                add(this@ParsedSymbol)
                children.forEach { addAll(it.flatten()) }
            }
        }
    }

    private companion object {
        private val CLASS_REGEXES = listOf(
            // Kotlin/Java core
            Regex("""^\s*(enum\s+class|annotation\s+class|class|interface|object)\s+([A-Za-z_][A-Za-z0-9_]*)\b"""),
            Regex("""^\s*(?:public|private|protected|internal|abstract|final|open|sealed|data|static|export|default|actual|expect|non-sealed)\s+(class|interface|enum|record)\s+([A-Za-z_][A-Za-z0-9_]*)\b"""),
            // Rust: [pub] struct/enum/trait Name
            Regex("""^\s*(?:pub(?:\([^)]*\))?\s+)?(struct|enum|trait)\s+([A-Za-z_][A-Za-z0-9_]*)\b"""),
            // Swift: [mods] struct/protocol Name
            Regex("""^\s*(?:public|private|internal|open|final|\s)*(struct|protocol)\s+([A-Za-z_][A-Za-z0-9_]*)\b"""),
            // C#: [mods] namespace/struct Name
            Regex("""^\s*(?:public|private|protected|internal|static|sealed|partial|\s)*(namespace|struct)\s+([A-Za-z_][A-Za-z0-9_.]*)\b"""),
            // Scala: [mods] trait Name / case class Name
            Regex("""^\s*(?:sealed|abstract|final|\s)*(trait)\s+([A-Za-z_][A-Za-z0-9_]*)\b"""),
            Regex("""^\s*(?:sealed|abstract|final|\s)*case\s+(class)\s+([A-Za-z_][A-Za-z0-9_]*)\b"""),
            // C/C++: [typedef] struct/union Name {
            Regex("""^\s*(?:typedef\s+)?(struct|union)\s+([A-Za-z_][A-Za-z0-9_]*)\s*\{"""),
            // PHP: trait Name
            Regex("""^\s*(?:abstract\s+)?(?:final\s+)?(trait)\s+([A-Za-z_][A-Za-z0-9_]*)\b"""),
            // Ruby: module Name
            Regex("""^\s*(module)\s+([A-Za-z_][A-Za-z0-9_:]*)\b""")
        )
        // Go: type Name struct/interface — reversed group order (group1=name, group2=kind)
        private val GO_TYPE_REGEX = Regex("""^\s*type\s+([A-Za-z_][A-Za-z0-9_]*)\s+(struct|interface)\b""")
        private val FUNCTION_REGEXES = listOf(
            // Kotlin: fun name(
            Regex("""^\s*(?:public|private|protected|internal|open|abstract|override|suspend|inline|operator|tailrec|external|infix|actual|expect|\s)*fun\s+([A-Za-z_][A-Za-z0-9_]*)\s*\("""),
            // JS/TS: function name(
            Regex("""^\s*(?:export\s+)?(?:async\s+)?function\s+([A-Za-z_][A-Za-z0-9_]*)\s*\("""),
            // JS/TS: const name = (...) =>
            Regex("""^\s*(?:export\s+)?(?:const|let|var)\s+([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(?:async\s*)?\([^)]*\)\s*=>"""),
            // Java: [modifiers] ReturnType name(
            Regex("""^\s*(?:public|private|protected|static|final|abstract|synchronized|native|default|async|\s)+(?:<[^>]+>\s*)?(?:[A-Za-z_][\w<>\[\],.?]*\s+)+([A-Za-z_][A-Za-z0-9_]*)\s*\([^;=]*\)\s*\{?"""),
            // Python: [async] def name(
            Regex("""^\s*(?:async\s+)?def\s+([A-Za-z_][A-Za-z0-9_]*)\s*\("""),
            // Go: func [receiver] name(
            Regex("""^\s*func\s+(?:\([^)]*\)\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*\("""),
            // Rust: [pub] [async] [unsafe] [const] fn name
            Regex("""^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:unsafe\s+)?(?:const\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*[<(]"""),
            // Swift: [modifiers] func name
            Regex("""^\s*(?:public|private|internal|open|static|class|override|mutating|\s)*func\s+([A-Za-z_][A-Za-z0-9_]*)\s*[<(]"""),
            // Scala/Groovy: [modifiers] def name
            Regex("""^\s*(?:override\s+)?(?:private|protected|\s)*def\s+([A-Za-z_][A-Za-z0-9_]*)\s*[(\[:]"""),
            // Shell: function name() { / name() {
            Regex("""^\s*function\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(?\)?\s*\{"""),
            Regex("""^([A-Za-z_][A-Za-z0-9_]*)\s*\(\)\s*\{"""),
            // Ruby: def [self.]name
            Regex("""^\s*def\s+(?:self\.)?([A-Za-z_][A-Za-z0-9_?!]*)\b""")
        )
        private val PROPERTY_REGEXES = listOf(
            // Kotlin: val/var name
            Regex("""^\s*(?:public|private|protected|internal|override|lateinit|const|open|final|actual|expect|\s)*(val|var)\s+([A-Za-z_][A-Za-z0-9_]*)\b"""),
            // JS/TS: const/let/var name =
            Regex("""^\s*(?:export\s+)?(const|let|var)\s+([A-Za-z_][A-Za-z0-9_]*)\s*="""),
            // Java: [modifiers] Type name = / ;
            Regex("""^\s*((?:public|private|protected|static|final|transient|volatile|\s)+(?:[A-Za-z_][\w<>\[\],.?]*\s+)+)([A-Za-z_][A-Za-z0-9_]*)\s*(?:=|;)"""),
            // Swift: [modifiers] let name
            Regex("""^\s*(?:public|private|internal|static|\s)*(let)\s+([A-Za-z_][A-Za-z0-9_]*)\b"""),
            // Rust: [pub] let/const/static [mut] name
            Regex("""^\s*(?:pub(?:\([^)]*\))?\s+)?(let|const|static)\s+(?:mut\s+)?([A-Za-z_][A-Za-z0-9_]*)\b"""),
            // Go: var/const name
            Regex("""^\s*(var|const)\s+([A-Za-z_][A-Za-z0-9_]*)\s""")
        )
        private val RESERVED_WORDS = setOf(
            "if", "for", "while", "switch", "catch", "when",
            "elif", "else", "except", "finally", "unless", "until",
            "yield", "assert", "del", "print", "exec"
        )
        private val STATEMENT_PREFIXES = setOf(
            "return ", "throw ", "new ", "if ", "for ", "while ", "switch ", "catch ", "when ",
            "import ", "from ", "require ", "include ", "use ", "#include ", "#define ",
            "raise ", "yield ", "assert ", "package ", "defer ", "go ", "del ",
            "puts ", "print ", "println ", "echo ", "printf "
        )
        // Ruby scope openers (class, module, def, do, begin, if/unless/while/until at statement level)
        private val RUBY_SCOPE_OPENER = Regex("""^\s*(?:class|module|def|do|begin|if|unless|while|until|case|for)\b""")
        private val SEARCHABLE_EXTENSIONS = SharedContract.workspaceSearchableExtensions
        private val IDENTIFIER_REGEX = Regex("""[A-Za-z_][A-Za-z0-9_]*""")
        private val PACKAGE_REGEX = Regex("""^\s*package\s+([A-Za-z_][\w.]*)""")
        private val EXTENDS_REGEX = Regex("""\bextends\s+([A-Za-z_][\w.]*)""")
        private val IMPLEMENTS_REGEX = Regex("""\bimplements\s+([A-Za-z_][\w.,\s]*)""")
        private val PRIMARY_PROPERTY_REGEX = Regex("""(?:val|var)\s+([A-Za-z_][A-Za-z0-9_]*):?""")
    }

    private data class ParsedTypeDeclaration(
        val name: String,
        val qualifiedName: String,
        val kind: String,
        val supertypes: List<String>,
        val properties: List<String>
    )
}
