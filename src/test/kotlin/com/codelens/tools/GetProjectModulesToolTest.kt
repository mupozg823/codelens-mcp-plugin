package com.codelens.tools

import com.codelens.CodeLensTestBase
import com.intellij.openapi.module.ModuleManager

class GetProjectModulesToolTest : CodeLensTestBase() {

    fun testListsCurrentProjectModule() {
        val moduleName = ModuleManager.getInstance(project).modules.first().name

        val response = GetProjectModulesTool().execute(emptyMap(), project)

        assertTrue(response.contains("\"success\":true"))
        assertTrue(response.contains("\"count\":"))
        assertTrue(response.contains("\"name\":\"$moduleName\""))
    }
}
