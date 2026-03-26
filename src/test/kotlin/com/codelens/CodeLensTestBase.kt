package com.codelens

import com.intellij.testFramework.fixtures.BasePlatformTestCase

abstract class CodeLensTestBase : BasePlatformTestCase() {
    override fun getTestDataPath(): String = "src/test/testData"
}
