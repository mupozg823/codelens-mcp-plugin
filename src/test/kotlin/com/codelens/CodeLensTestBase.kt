package com.codelens

import com.intellij.testFramework.fixtures.BasePlatformTestCase
import java.nio.file.Files
import java.nio.file.Path

abstract class CodeLensTestBase : BasePlatformTestCase() {
    private var previousSerenaHome: String? = null
    private var previousBackend: String? = null
    private var previousWorkspaceRoot: String? = null

    override fun setUp() {
        super.setUp()
        previousSerenaHome = System.getProperty("codelens.serena.home")
        previousBackend = System.getProperty("codelens.backend")
        previousWorkspaceRoot = System.getProperty("codelens.workspace.root")
        val testHome = Path.of(myFixture.tempDirPath, "serena-home")
        Files.createDirectories(testHome)
        System.setProperty("codelens.serena.home", testHome.toString())
    }

    override fun tearDown() {
        try {
            if (previousSerenaHome == null) {
                System.clearProperty("codelens.serena.home")
            } else {
                System.setProperty("codelens.serena.home", previousSerenaHome)
            }
            if (previousBackend == null) {
                System.clearProperty("codelens.backend")
            } else {
                System.setProperty("codelens.backend", previousBackend)
            }
            if (previousWorkspaceRoot == null) {
                System.clearProperty("codelens.workspace.root")
            } else {
                System.setProperty("codelens.workspace.root", previousWorkspaceRoot)
            }
        } finally {
            super.tearDown()
        }
    }

    override fun getTestDataPath(): String = "src/test/testData"
}
