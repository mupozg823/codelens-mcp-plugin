package com.codelens.services

import com.codelens.model.SymbolInfo
import com.codelens.model.SymbolKind
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile

/**
 * Adapter interface for language-specific PSI handling.
 * Each supported language implements this to map its PSI types to our model.
 */
interface LanguageAdapter {

    /** Language identifier (e.g., "JAVA", "kotlin", "Python") */
    val languageId: String

    /** Check if this adapter supports the given file */
    fun supports(psiFile: PsiFile): Boolean

    /** Extract top-level symbols from a file */
    fun extractSymbols(psiFile: PsiFile, depth: Int = 1): List<SymbolInfo>

    /** Classify a PSI element into our SymbolKind */
    fun classifyElement(element: PsiElement): SymbolKind?

    /** Check if an element is a "declaration" (as opposed to a reference) */
    fun isDeclaration(element: PsiElement): Boolean

    /** Get the body text of a symbol element */
    fun getBodyText(element: PsiElement): String?

    /** Build a human-readable signature */
    fun buildSignature(element: PsiElement): String
}
