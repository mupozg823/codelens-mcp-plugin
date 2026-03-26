package com.codelens.services

import com.codelens.model.SymbolInfo

/**
 * Service for symbol analysis operations.
 */
interface SymbolService {

    /**
     * Get an overview of symbols in a file or directory.
     * @param path File or directory path (absolute or relative to project root)
     * @param depth How deep to explore (1 = top-level only, 2 = includes nested)
     */
    fun getSymbolsOverview(path: String, depth: Int = 1): List<SymbolInfo>

    /**
     * Find a symbol by name.
     * @param name Symbol name (exact or partial match)
     * @param filePath Optional: limit search to a specific file
     * @param includeBody Whether to include the full source code body
     * @param exactMatch Whether to require exact name match
     */
    fun findSymbol(
        name: String,
        filePath: String? = null,
        includeBody: Boolean = false,
        exactMatch: Boolean = true
    ): List<SymbolInfo>
}
