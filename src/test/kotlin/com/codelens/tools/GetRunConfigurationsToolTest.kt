package com.codelens.tools

import com.codelens.CodeLensTestBase

class GetRunConfigurationsToolTest : CodeLensTestBase() {

    fun testEmptyConfigurations() {
        val response = GetRunConfigurationsTool().execute(emptyMap(), project)

        assertTrue(response.contains("\"success\":true"))
        assertTrue(response.contains("\"configurations\""))
        assertTrue(response.contains("\"count\":0"))
    }
}
