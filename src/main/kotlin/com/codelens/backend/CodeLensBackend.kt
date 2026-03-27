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

    fun getTypeHierarchy(
        fullyQualifiedName: String,
        hierarchyType: String = "both",
        depth: Int = 1
    ): Map<String, Any?>

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
