# CodeLens MCP Plugin - Full Source Code Analysis

## Overview
Complete source code extraction and analysis of the CodeLens MCP plugin architecture, focusing on tool implementations, backend interfaces, and service layer.

---

## 1. TOOL IMPLEMENTATIONS (Complete Source)

### 1.1 TypeHierarchyTool.kt (30 lines)
**Location:** `/src/main/kotlin/com/codelens/tools/TypeHierarchyTool.kt`

```kotlin
package com.codelens.tools

import com.codelens.backend.CodeLensBackendProvider
import com.intellij.openapi.project.Project

class TypeHierarchyTool : BaseMcpTool() {
    override val toolName = "get_type_hierarchy"
    override val description = "Get the type hierarchy (supertypes and subtypes) for a class"
    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "fully_qualified_name" to mapOf(
                "type" to "string",
                "description" to "Fully qualified class name (e.g., com.example.MyClass)"
            )
        ),
        "required" to listOf("fully_qualified_name")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        return try {
            val fqn = requireString(args, "fully_qualified_name")
            val result = CodeLensBackendProvider.getBackend(project).getTypeHierarchy(fqn)
            successResponse(result)
        } catch (e: Exception) {
            errorResponse("Failed to get type hierarchy: ${e.message}")
        }
    }
}
```

**KEY FINDINGS:**
- Single parameter: `fully_qualified_name` (required)
- NO `depth` parameter - depth is hardcoded in backend's getTypeHierarchy()
- NO `hierarchy_type` parameter - always returns both supertypes and subtypes
- Backend call: `getTypeHierarchy(fqn)` returns `Map<String, Any?>`

---

### 1.2 JetBrainsTypeHierarchyTool.kt (20 lines)
**Location:** `/src/main/kotlin/com/codelens/tools/JetBrainsTypeHierarchyTool.kt`

```kotlin
package com.codelens.tools

import com.intellij.openapi.project.Project

class JetBrainsTypeHierarchyTool : BaseMcpTool() {

    private val delegate = TypeHierarchyTool()

    override val toolName = "jet_brains_type_hierarchy"

    override val description = """
        Retrieve a type hierarchy using the JetBrains backend.
        This is the Serena-compatible JetBrains alias for get_type_hierarchy.
    """.trimIndent()

    override val inputSchema = delegate.inputSchema

    override fun execute(args: Map<String, Any?>, project: Project): String = delegate.execute(args, project)
}
```

**KEY FINDINGS:**
- Pure delegation pattern to TypeHierarchyTool
- Identical input schema
- Just a naming alias for Serena compatibility

---

### 1.3 FindSymbolTool.kt (77 lines)
**Location:** `/src/main/kotlin/com/codelens/tools/FindSymbolTool.kt`

```kotlin
package com.codelens.tools

import com.codelens.backend.CodeLensBackendProvider
import com.intellij.openapi.project.Project

/**
 * MCP Tool: find_symbol
 *
 * Finds a symbol by name, optionally including its full source body.
 * Equivalent to Serena's find_symbol tool.
 */
class FindSymbolTool : BaseMcpTool() {

    override val toolName = "find_symbol"

    override val description = """
        Find a symbol (class, function, variable) by name.
        Can search within a specific file or across the entire project.
        Optionally returns the full source code body of the symbol.
    """.trimIndent()

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "name" to mapOf(
                "type" to "string",
                "description" to "Symbol name to search for"
            ),
            "name_path" to mapOf(
                "type" to "string",
                "description" to "Optional disambiguated name path such as Outer/helper"
            ),
            "file_path" to mapOf(
                "type" to "string",
                "description" to "Optional: limit search to a specific file"
            ),
            "include_body" to mapOf(
                "type" to "boolean",
                "description" to "Whether to include the full source code body",
                "default" to false
            ),
            "exact_match" to mapOf(
                "type" to "boolean",
                "description" to "Whether to require exact name match (false for substring match)",
                "default" to true
            )
        )
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        val name = optionalString(args, "name_path") ?: requireString(args, "name")
        val filePath = optionalString(args, "file_path")
        val includeBody = optionalBoolean(args, "include_body", false)
        val exactMatch = optionalBoolean(args, "exact_match", true)

        return try {
            val symbols = CodeLensBackendProvider.getBackend(project)
                .findSymbol(name, filePath, includeBody, exactMatch)

            if (symbols.isEmpty()) {
                val scope = filePath?.let { "in '$it'" } ?: "in project"
                successResponse(mapOf(
                    "symbols" to emptyList<Any>(),
                    "message" to "Symbol '$name' not found $scope"
                ))
            } else {
                successResponse(mapOf(
                    "symbols" to symbols.map { it.toMap() },
                    "count" to symbols.size
                ))
            }
        } catch (e: Exception) {
            errorResponse("Failed to find symbol: ${e.message}")
        }
    }
}
```

**KEY FINDINGS - MISSING PARAMETERS:**
- NO `search_deps` parameter (for searching in project dependencies)
- NO `max_matches` parameter (results are limited in SymbolServiceImpl at hardcoded 50)
- NO `include_info` parameter (for hover-like docstring/signature)
- NO `depth` parameter (for nested symbol search)
- Backend call: `findSymbol(name, filePath, includeBody, exactMatch)` → `List<SymbolInfo>`

---

### 1.4 SearchForPatternTool.kt (73 lines)
**Location:** `/src/main/kotlin/com/codelens/tools/SearchForPatternTool.kt`

```kotlin
package com.codelens.tools

import com.codelens.backend.CodeLensBackendProvider
import com.intellij.openapi.project.Project

/**
 * MCP Tool: search_for_pattern
 *
 * Regex-based pattern search across project files.
 * Equivalent to Serena's search_for_pattern tool.
 */
class SearchForPatternTool : BaseMcpTool() {

    override val toolName = "search_for_pattern"

    override val description = """
        Search for a regex pattern across project files.
        Returns matching files, line numbers, and matched content.
        Optionally filter by file extension and include surrounding context.
    """.trimIndent()

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "pattern" to mapOf(
                "type" to "string",
                "description" to "Regex pattern to search for"
            ),
            "file_glob" to mapOf(
                "type" to "string",
                "description" to "Optional file filter (e.g., '*.kt', '*.java')"
            ),
            "max_results" to mapOf(
                "type" to "integer",
                "description" to "Maximum number of results",
                "default" to 50
            ),
            "context_lines" to mapOf(
                "type" to "integer",
                "description" to "Number of context lines before/after each match",
                "default" to 0
            )
        ),
        "required" to listOf("pattern")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        val pattern = requireString(args, "pattern")
        val fileGlob = optionalString(args, "file_glob")
        val maxResults = optionalInt(args, "max_results", 50)
        val contextLines = optionalInt(args, "context_lines", 0)

        return try {
            val results = CodeLensBackendProvider.getBackend(project)
                .searchForPattern(pattern, fileGlob, maxResults, contextLines)

            if (results.isEmpty()) {
                successResponse(mapOf(
                    "results" to emptyList<Any>(),
                    "message" to "No matches found for pattern: $pattern"
                ))
            } else {
                successResponse(mapOf(
                    "results" to results.map { it.toMap() },
                    "count" to results.size
                ))
            }
        } catch (e: Exception) {
            errorResponse("Search failed: ${e.message}")
        }
    }
}
```

**KEY FINDINGS - MISSING PARAMETERS:**
- NO `context_lines_before` / `context_lines_after` (unified into single `context_lines`)
- NO `paths_include_glob` / `paths_exclude_glob` (only have `file_glob`)
- NO `max_answer_chars` parameter
- NO `restrict_search_to_code_files` parameter
- Backend call: `searchForPattern(pattern, fileGlob, maxResults, contextLines)` → `List<SearchResult>`

---

### 1.5 GetSymbolsOverviewTool.kt (61 lines)
**Location:** `/src/main/kotlin/com/codelens/tools/GetSymbolsOverviewTool.kt`

```kotlin
package com.codelens.tools

import com.codelens.backend.CodeLensBackendProvider
import com.intellij.openapi.project.Project

/**
 * MCP Tool: get_symbols_overview
 *
 * Returns a structural overview of symbols in a file or directory.
 * Equivalent to Serena's get_symbols_overview tool.
 */
class GetSymbolsOverviewTool : BaseMcpTool() {

    override val toolName = "get_symbols_overview"

    override val description = """
        Get an overview of code symbols (classes, functions, variables) in a file or directory.
        Returns symbol names, kinds, line numbers, and signatures.
        Use depth=1 for top-level only, depth=2 to include nested symbols.
    """.trimIndent()

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "path" to mapOf(
                "type" to "string",
                "description" to "File or directory path (absolute or relative to project root)"
            ),
            "depth" to mapOf(
                "type" to "integer",
                "description" to "How deep to explore: 1=top-level only, 2=includes nested members",
                "default" to 1
            )
        ),
        "required" to listOf("path")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        val path = requireString(args, "path")
        val depth = optionalInt(args, "depth", 1)

        return try {
            val symbols = CodeLensBackendProvider.getBackend(project).getSymbolsOverview(path, depth)

            if (symbols.isEmpty()) {
                successResponse(mapOf(
                    "symbols" to emptyList<Any>(),
                    "message" to "No symbols found in '$path'"
                ))
            } else {
                successResponse(mapOf(
                    "symbols" to symbols.map { it.toMap() },
                    "count" to symbols.size
                ))
            }
        } catch (e: Exception) {
            errorResponse("Failed to get symbols overview: ${e.message}")
        }
    }
}
```

**KEY FINDINGS - MISSING PARAMETERS:**
- NO `max_answer_chars` parameter
- NO `include_file_documentation` parameter (for file-level docstrings)
- Backend call: `getSymbolsOverview(path, depth)` → `List<SymbolInfo>`

---

### 1.6 ReplaceContentTool.kt (65 lines)
**Location:** `/src/main/kotlin/com/codelens/tools/ReplaceContentTool.kt`

```kotlin
package com.codelens.tools

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiManager

class ReplaceContentTool : BaseMcpTool() {
    override val toolName = "replace_content"
    override val description = "Replace all occurrences of a string with another string in a file"
    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "relative_path" to mapOf("type" to "string", "description" to "Relative path to the file"),
            "find" to mapOf("type" to "string", "description" to "String to find"),
            "replace" to mapOf("type" to "string", "description" to "Replacement string"),
            "first_only" to mapOf("type" to "boolean", "description" to "If true, replace only the first occurrence (default: false)")
        ),
        "required" to listOf("relative_path", "find", "replace")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        return try {
            val relativePath = requireString(args, "relative_path")
            val find = requireString(args, "find")
            val replace = requireString(args, "replace")
            val firstOnly = optionalBoolean(args, "first_only", false)

            val basePath = project.basePath ?: return errorResponse("No project base path found")
            val filePath = if (relativePath.startsWith("/")) relativePath else "$basePath/$relativePath"

            val psiFile = PsiManager.getInstance(project).findFile(
                com.intellij.openapi.vfs.LocalFileSystem.getInstance().findFileByPath(filePath)
                    ?: return errorResponse("File not found: $relativePath")
            ) ?: return errorResponse("Cannot open file: $relativePath")

            var replacementCount = 0
            ApplicationManager.getApplication().invokeAndWait {
                WriteCommandAction.runWriteCommandAction(project) {
                    val document = com.codelens.util.PsiUtils.getDocument(psiFile)
                        ?: throw IllegalArgumentException("Cannot get document")
                    val content = document.text
                    val newContent = if (firstOnly) {
                        val index = content.indexOf(find)
                        if (index >= 0) {
                            replacementCount = 1
                            content.replaceFirst(find, replace)
                        } else {
                            content
                        }
                    } else {
                        replacementCount = content.split(find).size - 1
                        content.replace(find, replace)
                    }
                    document.setText(newContent)
                }
            }

            successResponse(mapOf("success" to true, "file_path" to relativePath, "replacements" to replacementCount))
        } catch (e: Exception) {
            errorResponse("Failed to replace content: ${e.message}")
        }
    }
}
```

**KEY FINDINGS - MISSING PARAMETERS:**
- NO `allow_multiple_occurrences` parameter
- NO `mode` parameter (literal vs regex)
- Only supports literal string find/replace, no regex patterns
- Uses `firstOnly` boolean instead of `allow_multiple_occurrences`
- Counts replacements internally via string split

---

### 1.7 FindReferencingSymbolsTool.kt (70 lines)
**Location:** `/src/main/kotlin/com/codelens/tools/FindReferencingSymbolsTool.kt`

```kotlin
package com.codelens.tools

import com.codelens.backend.CodeLensBackendProvider
import com.intellij.openapi.project.Project

/**
 * MCP Tool: find_referencing_symbols
 *
 * Finds all locations in the codebase that reference a given symbol.
 * Equivalent to Serena's find_referencing_symbols tool.
 */
class FindReferencingSymbolsTool : BaseMcpTool() {

    override val toolName = "find_referencing_symbols"

    override val description = """
        Find all locations that reference a given symbol.
        Shows the file, line, containing symbol, and context for each reference.
        Useful for understanding how a symbol is used across the codebase.
    """.trimIndent()

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "symbol_name" to mapOf(
                "type" to "string",
                "description" to "Name of the symbol to find references for"
            ),
            "name_path" to mapOf(
                "type" to "string",
                "description" to "Optional disambiguated name path such as Outer/helper"
            ),
            "file_path" to mapOf(
                "type" to "string",
                "description" to "Optional: file where the symbol is defined (for disambiguation)"
            ),
            "max_results" to mapOf(
                "type" to "integer",
                "description" to "Maximum number of results to return",
                "default" to 50
            )
        )
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        val symbolName = optionalString(args, "name_path") ?: requireString(args, "symbol_name")
        val filePath = optionalString(args, "file_path")
        val maxResults = optionalInt(args, "max_results", 50)

        return try {
            val references = CodeLensBackendProvider.getBackend(project)
                .findReferencingSymbols(symbolName, filePath, maxResults)

            if (references.isEmpty()) {
                successResponse(mapOf(
                    "references" to emptyList<Any>(),
                    "message" to "No references found for '$symbolName'"
                ))
            } else {
                successResponse(mapOf(
                    "references" to references.map { it.toMap() },
                    "count" to references.size
                ))
            }
        } catch (e: Exception) {
            errorResponse("Failed to find references: ${e.message}")
        }
    }
}
```

**KEY FINDINGS - MISSING PARAMETERS:**
- NO `max_answer_chars` parameter
- Backend call: `findReferencingSymbols(symbolName, filePath, maxResults)` → `List<ReferenceInfo>`

---

## 2. BACKEND INTERFACE (CodeLensBackend.kt)

**Location:** `/src/main/kotlin/com/codelens/backend/CodeLensBackend.kt`

```kotlin
package com.codelens.backend

import com.codelens.model.FileEntry
import com.codelens.model.FileReadResult
import com.codelens.model.ModificationResult
import com.codelens.model.ReferenceInfo
import com.codelens.model.SearchResult
import com.codelens.model.SymbolInfo
import com.codelens.services.RenameScope

interface CodeLensBackend {
    val backendId: String
    val languageBackendName: String

    fun getSymbolsOverview(path: String, depth: Int = 1): List<SymbolInfo>

    fun findSymbol(
        name: String,
        filePath: String? = null,
        includeBody: Boolean = false,
        exactMatch: Boolean = true
    ): List<SymbolInfo>

    fun findReferencingSymbols(
        symbolName: String,
        filePath: String? = null,
        maxResults: Int = 50
    ): List<ReferenceInfo>

    fun getTypeHierarchy(fullyQualifiedName: String): Map<String, Any?>

    fun replaceSymbolBody(
        symbolName: String,
        filePath: String,
        newBody: String
    ): ModificationResult

    fun insertAfterSymbol(
        symbolName: String,
        filePath: String,
        content: String
    ): ModificationResult

    fun insertBeforeSymbol(
        symbolName: String,
        filePath: String,
        content: String
    ): ModificationResult

    fun renameSymbol(
        symbolName: String,
        filePath: String,
        newName: String,
        scope: RenameScope = RenameScope.PROJECT
    ): ModificationResult

    fun readFile(path: String, startLine: Int? = null, endLine: Int? = null): FileReadResult

    fun listDirectory(path: String, recursive: Boolean = false): List<FileEntry>

    fun findFiles(pattern: String, baseDir: String? = null): List<String>

    fun searchForPattern(
        pattern: String,
        fileGlob: String? = null,
        maxResults: Int = 50,
        contextLines: Int = 0
    ): List<SearchResult>
}
```

**KEY FINDINGS:**
- `getTypeHierarchy(fullyQualifiedName: String)` signature is MINIMAL
- NO depth parameter in backend interface signature
- NO hierarchy_type parameter (super, sub, or both)
- Returns Map<String, Any?> - structure defined in implementation

---

## 3. JETBRAINS BACKEND IMPLEMENTATION (Partial - Type Hierarchy Focus)

**Location:** `/src/main/kotlin/com/codelens/backend/jetbrains/JetBrainsCodeLensBackend.kt`

### getTypeHierarchy Implementation (lines 57-72):

```kotlin
override fun getTypeHierarchy(fullyQualifiedName: String): Map<String, Any?> {
    return ReadAction.compute<Map<String, Any?>, Exception> {
        val psiClass = findPsiClass(fullyQualifiedName)
            ?: return@compute mapOf("error" to "Class not found: $fullyQualifiedName")

        mapOf(
            "class_name" to psiClass.name,
            "fully_qualified_name" to psiClass.qualifiedName,
            "kind" to getClassKind(psiClass),
            "supertypes" to getSupertypes(psiClass),
            "subtypes" to getSubtypes(psiClass),
            "members" to getMembers(psiClass),
            "type_parameters" to getTypeParameters(psiClass)
        )
    }
}
```

### getSupertypes (lines 202-214):

```kotlin
private fun getSupertypes(psiClass: PsiClass): List<Map<String, String>> {
    return try {
        psiClass.supers.map { superClass ->
            mapOf(
                "name" to (superClass.name ?: ""),
                "qualified_name" to (superClass.qualifiedName ?: ""),
                "kind" to if (superClass.isInterface) "interface" else "class"
            )
        }
    } catch (_: Exception) {
        emptyList()
    }
}
```

### getSubtypes (lines 216-228):

```kotlin
private fun getSubtypes(psiClass: PsiClass): List<Map<String, String>> {
    return try {
        ClassInheritorsSearch.search(psiClass, GlobalSearchScope.projectScope(project), true)
            .map { subClass ->
                mapOf(
                    "name" to (subClass.name ?: ""),
                    "qualified_name" to (subClass.qualifiedName ?: "")
                )
            }
    } catch (_: Exception) {
        emptyList()
    }
}
```

**KEY FINDINGS:**
- `ClassInheritorsSearch.search()` is used for subtypes, limited by `true` (shallow search)
- NO depth parameter passed to ClassInheritorsSearch - depth is hardcoded to 1
- Always returns direct supertypes/subtypes only
- Third parameter `true` in ClassInheritorsSearch likely means "include self" or shallow mode

---

## 4. SERVICE IMPLEMENTATIONS

### 4.1 SymbolServiceImpl.kt (192 lines)

**Key Methods:**

#### getSymbolsOverview (lines 40-52):
```kotlin
override fun getSymbolsOverview(path: String, depth: Int): List<SymbolInfo> {
    return DumbService.getInstance(project).runReadActionInSmartMode<List<SymbolInfo>> {
        val resolvedPath = resolvePath(path)
        val virtualFile = PsiUtils.resolveVirtualFile(resolvedPath)
            ?: return@runReadActionInSmartMode emptyList()

        if (virtualFile.isDirectory) {
            getDirectorySymbols(virtualFile, depth)
        } else {
            getFileSymbols(virtualFile, depth)
        }
    }
}
```

#### findSymbolInProject (lines 140-181):
- Has hardcoded result limit: `if (results.size >= 50) break`
- Line 177: `// Limit results` comment shows intentional but crude limiting
- No search_deps parameter implementation

**CRITICAL HARDCODES:**
- Line 177: Results hardcoded to max 50 (no maxMatches parameter)
- Line 150-154: Only searches specific file extensions (java, kt, py, js, ts)

---

### 4.2 ReferenceServiceImpl.kt (118 lines)

**Key Method - findReferencingSymbols (lines 14-57):**

```kotlin
override fun findReferencingSymbols(
    symbolName: String,
    filePath: String?,
    maxResults: Int
): List<ReferenceInfo> {
    return DumbService.getInstance(project).runReadActionInSmartMode<List<ReferenceInfo>> {
        val targetElement = resolveSymbol(symbolName, filePath) ?: return@runReadActionInSmartMode emptyList()
        val scope = GlobalSearchScope.projectScope(project)
        val references = ReferencesSearch.search(targetElement, scope)
            .findAll()
            .take(maxResults)
        // ... maps to ReferenceInfo
    }
}
```

**KEY FINDINGS:**
- Uses `ReferencesSearch.search()` from IntelliJ
- Uses `.take(maxResults)` for limiting results
- No max_answer_chars filtering
- Maps to ReferenceInfo with: filePath, line, column, containingSymbol, context, isWrite

---

## 5. JET BRAINS ALIAS TOOLS

All are pure delegation patterns:

### JetBrainsFindSymbolTool.kt (20 lines)
```kotlin
class JetBrainsFindSymbolTool : BaseMcpTool() {
    private val delegate = FindSymbolTool()
    override val toolName = "jet_brains_find_symbol"
    override val inputSchema = delegate.inputSchema
    override fun execute(args, project) = delegate.execute(args, project)
}
```

### JetBrainsGetSymbolsOverviewTool.kt (20 lines)
```kotlin
class JetBrainsGetSymbolsOverviewTool : BaseMcpTool() {
    private val delegate = GetSymbolsOverviewTool()
    override val toolName = "jet_brains_get_symbols_overview"
    override val inputSchema = delegate.inputSchema
    override fun execute(args, project) = delegate.execute(args, project)
}
```

### JetBrainsFindReferencingSymbolsTool.kt (20 lines)
```kotlin
class JetBrainsFindReferencingSymbolsTool : BaseMcpTool() {
    private val delegate = FindReferencingSymbolsTool()
    override val toolName = "jet_brains_find_referencing_symbols"
    override val inputSchema = delegate.inputSchema
    override fun execute(args, project) = delegate.execute(args, project)
}
```

---

## 6. BASE CLASS - BaseMcpTool.kt

```kotlin
abstract class BaseMcpTool {
    abstract val toolName: String
    abstract val description: String
    abstract val inputSchema: Map<String, Any>
    abstract fun execute(args: Map<String, Any?>, project: Project): String
    
    // Helper methods:
    protected fun requireString(args, key): String
    protected fun optionalString(args, key): String?
    protected fun optionalInt(args, key, default): Int
    protected fun optionalBoolean(args, key, default): Boolean
    protected fun successResponse(data): String
    protected fun errorResponse(message): String
}
```

---

## 7. MODEL - TypeHierarchyInfo.kt

```kotlin
data class TypeHierarchyInfo(
    val name: String,
    val kind: SymbolKind,
    val filePath: String?,
    val line: Int,
    val signature: String,
    val depth: Int
) {
    fun toMap(): Map<String, Any?> = buildMap {
        put("name", name)
        put("kind", kind.displayName)
        if (filePath != null) put("file", filePath)
        put("line", line)
        put("signature", signature)
        put("depth", depth)
    }
}
```

---

## SUMMARY: CURRENT ARCHITECTURE

### Tool Flow:
1. User calls tool via MCP (e.g., `jet_brains_find_symbol`)
2. Alias tool delegates to main tool (e.g., FindSymbolTool)
3. Tool validates input schema and calls backend
4. Backend delegates to service (SymbolService, ReferenceService, etc.)
5. Service uses IntelliJ PSI APIs to fetch data
6. Result mapped to SymbolInfo/ReferenceInfo/etc.
7. Serialized to JSON response

### MISSING/HARDCODED PARAMETERS:

| Tool | Missing Parameters | Current Hardcodes |
|------|------------------|-------------------|
| **TypeHierarchyTool** | `depth`, `hierarchy_type` | Always returns both super+sub, no depth control |
| **FindSymbolTool** | `search_deps`, `max_matches`, `include_info` | max 50 results hardcoded in service |
| **SearchForPatternTool** | `context_lines_before/after`, `paths_include/exclude_glob`, `max_answer_chars` | Single `context_lines`, single `file_glob` |
| **GetSymbolsOverviewTool** | `max_answer_chars`, `include_file_documentation` | None documented |
| **ReplaceContentTool** | `mode` (literal/regex), `allow_multiple_occurrences` | Only literal string replace, `first_only` boolean only |
| **FindReferencingSymbolsTool** | `max_answer_chars` | maxResults honored at tool level |
