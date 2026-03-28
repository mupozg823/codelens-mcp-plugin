package com.codelens.services

import com.codelens.model.ModificationResult

/**
 * Service for symbol-level code modifications.
 */
interface ModificationService {

    /**
     * Replace the body of a symbol with new code.
     */
    fun replaceSymbolBody(
        symbolName: String,
        filePath: String,
        newBody: String
    ): ModificationResult

    /**
     * Insert code after a symbol.
     */
    fun insertAfterSymbol(
        symbolName: String,
        filePath: String,
        content: String
    ): ModificationResult

    /**
     * Insert code before a symbol.
     */
    fun insertBeforeSymbol(
        symbolName: String,
        filePath: String,
        content: String
    ): ModificationResult

    /**
     * Rename a symbol across the project.
     */
    fun renameSymbol(
        symbolName: String,
        filePath: String,
        newName: String,
        scope: RenameScope = RenameScope.PROJECT
    ): ModificationResult
}
