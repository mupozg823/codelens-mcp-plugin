package com.codelens.services

import com.codelens.model.SearchResult

/**
 * Service for pattern-based code search.
 */
interface SearchService {

    /**
     * Search for a regex pattern across project files.
     * @param pattern Regex pattern to search for
     * @param fileGlob Optional file filter (e.g., "*.kt", "*.java")
     * @param maxResults Maximum number of results
     * @param contextLines Number of context lines before/after each match
     */
    fun searchForPattern(
        pattern: String,
        fileGlob: String? = null,
        maxResults: Int = 50,
        contextLines: Int = 0
    ): List<SearchResult>
}
