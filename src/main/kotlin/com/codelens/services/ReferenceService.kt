package com.codelens.services

import com.codelens.model.ReferenceInfo

/**
 * Service for finding symbol references across the codebase.
 */
interface ReferenceService {

    /**
     * Find all locations that reference a given symbol.
     * @param symbolName Name of the symbol to find references for
     * @param filePath Optional: file where the symbol is defined (for disambiguation)
     * @param maxResults Maximum number of results to return
     */
    fun findReferencingSymbols(
        symbolName: String,
        filePath: String? = null,
        maxResults: Int = 50
    ): List<ReferenceInfo>
}
