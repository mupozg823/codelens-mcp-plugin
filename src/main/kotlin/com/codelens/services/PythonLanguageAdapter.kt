package com.codelens.services

import com.codelens.model.SymbolInfo
import com.codelens.model.SymbolKind
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile

/**
 * Placeholder for future Python PSI support.
 *
 * The core plugin avoids direct references to optional Python classes so it can
 * pass verification against IDEs where the Python plugin is not available.
 */
class PythonLanguageAdapter : LanguageAdapter {

    override val languageId: String = "Python"

    override fun supports(psiFile: PsiFile): Boolean = false

    override fun extractSymbols(psiFile: PsiFile, depth: Int): List<SymbolInfo> = emptyList()

    override fun classifyElement(element: PsiElement): SymbolKind? = null

    override fun isDeclaration(element: PsiElement): Boolean = false

    override fun getBodyText(element: PsiElement): String? = null

    override fun buildSignature(element: PsiElement): String = element.text
}
