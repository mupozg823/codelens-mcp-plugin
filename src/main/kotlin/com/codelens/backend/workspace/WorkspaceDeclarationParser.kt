package com.codelens.backend.workspace

import com.codelens.model.SymbolInfo
import com.codelens.model.SymbolKind
import com.codelens.model.SharedContract
import java.nio.file.Path

internal data class ParsedSymbol(
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

internal data class ParsedTypeDeclaration(
    val name: String,
    val qualifiedName: String,
    val kind: String,
    val supertypes: List<String>,
    val properties: List<String>
)

internal val CLASS_REGEXES = listOf(
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
internal val GO_TYPE_REGEX = Regex("""^\s*type\s+([A-Za-z_][A-Za-z0-9_]*)\s+(struct|interface)\b""")

internal val FUNCTION_REGEXES = listOf(
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
    Regex("""^\s*(?:override\s+)?(?:private|protected|\s)*def\s+([A-Za-z_][A-Za-z0-9_]*)\s*[(\[:] """),
    // Shell: function name() { / name() {
    Regex("""^\s*function\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(?\)?\s*\{"""),
    Regex("""^([A-Za-z_][A-Za-z0-9_]*)\s*\(\)\s*\{"""),
    // Ruby: def [self.]name
    Regex("""^\s*def\s+(?:self\.)?([A-Za-z_][A-Za-z0-9_?!]*)\b""")
)

internal val PROPERTY_REGEXES = listOf(
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

internal val RESERVED_WORDS = setOf(
    "if", "for", "while", "switch", "catch", "when",
    "elif", "else", "except", "finally", "unless", "until",
    "yield", "assert", "del", "print", "exec"
)

internal val STATEMENT_PREFIXES = setOf(
    "return ", "throw ", "new ", "if ", "for ", "while ", "switch ", "catch ", "when ",
    "import ", "from ", "require ", "include ", "use ", "#include ", "#define ",
    "raise ", "yield ", "assert ", "package ", "defer ", "go ", "del ",
    "puts ", "print ", "println ", "echo ", "printf "
)

// Ruby scope openers (class, module, def, do, begin, if/unless/while/until at statement level)
internal val RUBY_SCOPE_OPENER = Regex("""^\s*(?:class|module|def|do|begin|if|unless|while|until|case|for)\b""")

internal val SEARCHABLE_EXTENSIONS = SharedContract.workspaceSearchableExtensions

internal val IDENTIFIER_REGEX = Regex("""[A-Za-z_][A-Za-z0-9_]*""")

internal val PACKAGE_REGEX = Regex("""^\s*package\s+([A-Za-z_][\w.]*)""")

internal val EXTENDS_REGEX = Regex("""\bextends\s+([A-Za-z_][\w.]*)""")

internal val IMPLEMENTS_REGEX = Regex("""\bimplements\s+([A-Za-z_][\w.,\s]*)""")

internal val PRIMARY_PROPERTY_REGEX = Regex("""(?:val|var)\s+([A-Za-z_][A-Za-z0-9_]*):?""")

internal fun parseDeclaration(
    line: String,
    path: Path,
    index: Int,
    lines: List<String>,
    includeBodies: Boolean,
    relativize: (Path) -> String
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

internal fun extractBody(lines: List<String>, startIndex: Int): String {
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
internal fun extractIndentBody(lines: List<String>, startIndex: Int): String {
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
internal fun extractEndKeywordBody(lines: List<String>, startIndex: Int): String {
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

internal fun parseTypeDeclaration(path: Path, relativize: (Path) -> String): ParsedTypeDeclaration? {
    val lines = runCatching { path.toFile().readLines() }.getOrNull() ?: return null
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

internal fun classKindForDeclaration(line: String, token: String): String = when {
    line.contains("data class") -> "data_class"
    classKindFor(token) == SymbolKind.INTERFACE -> "interface"
    classKindFor(token) == SymbolKind.ENUM -> "enum"
    classKindFor(token) == SymbolKind.OBJECT -> "object"
    else -> "class"
}

internal fun extractSupertypes(line: String): List<String> {
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

internal fun normalizeTypeName(raw: String): String {
    val trimmed = raw.trim().substringBefore(" where ").substringBefore("<")
    return trimmed.substringBefore("(").substringAfterLast('.')
}

internal fun extractPrimaryProperties(line: String): List<String> {
    val parameterBlock = line.substringAfter("(", "").substringBeforeLast(")", "")
    if (parameterBlock.isEmpty()) return emptyList()
    return parameterBlock.split(',')
        .map { it.trim() }
        .mapNotNull { parameter ->
            PRIMARY_PROPERTY_REGEX.find(parameter)?.groupValues?.getOrNull(1)
        }
}

internal fun classKindFor(token: String): SymbolKind = when (token.trim()) {
    "interface" -> SymbolKind.INTERFACE
    "enum", "enum class" -> SymbolKind.ENUM
    "object" -> SymbolKind.OBJECT
    "annotation class" -> SymbolKind.ANNOTATION
    "trait", "protocol" -> SymbolKind.INTERFACE
    "namespace", "module" -> SymbolKind.MODULE
    "struct", "union", "record" -> SymbolKind.CLASS
    else -> SymbolKind.CLASS
}
