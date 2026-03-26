package com.codelens.tools

import com.codelens.CodeLensTestBase
import com.intellij.openapi.fileEditor.FileEditorManager

class GetOpenFilesToolTest : CodeLensTestBase() {

    fun testReportsOpenEditorFiles() {
        val psiFile = myFixture.addFileToProject("OpenMe.java", "class OpenMe {}")
        FileEditorManager.getInstance(project).openFile(psiFile.virtualFile, true)

        val response = GetOpenFilesTool().execute(emptyMap(), project)

        assertTrue(response.contains("\"success\":true"))
        assertTrue(response.contains("\"count\":"))
        assertTrue(response.contains("\"name\":\"OpenMe.java\""))
        assertTrue(response.contains("\"is_current\":true"))
    }
}
