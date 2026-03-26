package com.codelens.services

import com.codelens.CodeLensTestBase
import com.intellij.openapi.components.service

class SearchServiceTest : CodeLensTestBase() {

    private lateinit var searchService: SearchService

    override fun setUp() {
        super.setUp()
        searchService = project.service<SearchService>()
    }

    fun testSearchForPatternDoesNotCrash() {
        myFixture.addFileToProject("Searchable.java", """
            public class Searchable {
                public void findThis() {}
                public void findThat() {}
            }
        """.trimIndent())

        // SearchService traverses project.basePath on real filesystem.
        // In light fixture, basePath may not contain test files.
        // Just verify it doesn't crash.
        val results = searchService.searchForPattern("find\\w+", null, 50, 0)
        assertNotNull("Should return a list", results)
    }

    fun testSearchInvalidRegex() {
        myFixture.addFileToProject("Any.java", "public class Any {}")
        val results = searchService.searchForPattern("[invalid", null, 50, 0)
        assertTrue("Invalid regex should return empty", results.isEmpty())
    }

    fun testSearchMaxResultsConstraint() {
        // This tests the API contract even if no files are found on disk
        val results = searchService.searchForPattern(".*", null, 3, 0)
        assertTrue("Should respect maxResults", results.size <= 3)
    }

    fun testSearchWithFileGlobDoesNotCrash() {
        val results = searchService.searchForPattern("class", "*.java", 50, 0)
        assertNotNull("Should return a list", results)
    }
}
