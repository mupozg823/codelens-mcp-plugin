package com.codelens.services

import com.codelens.model.SymbolInfo
import com.codelens.model.SymbolKind
import com.codelens.util.PsiUtils
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.sh.psi.ShFile
import com.intellij.sh.psi.ShFunctionDefinition

/**
 * Shell Script PSI adapter.
 * Uses Shell Script PSI classes for function definition extraction.
 * Only loaded when the Shell Script plugin is available (bundled in IntelliJ).
 */
class ShellLanguageAdapter : LanguageAdapter {

    override val languageId: String = "Shell"

    override fun supports(psiFile: PsiFile): Boolean = psiFile is ShFile

    override fun extractSymbols(psiFile: PsiFile, depth: Int): List<SymbolInfo> {
        if (psiFile !is ShFile) return emptyList()

        val filePath = psiFile.virtualFile?.path ?: ""
        val functions = PsiTreeUtil.findChildrenOfType(psiFile, ShFunctionDefinition::class.java)

        return functions.mapNotNull { func ->
            val name = func.name ?: return@mapNotNull null
            SymbolInfo(
                name = name,
                kind = SymbolKind.FUNCTION,
                filePath = filePath,
                line = PsiUtils.getLineNumber(func),
                column = PsiUtils.getColumnNumber(func),
                signature = "function $name()",
                documentation = PsiUtils.extractDocumentation(func)
            )
        }
    }

    override fun classifyElement(element: PsiElement): SymbolKind? = when (element) {
        is ShFunctionDefinition -> SymbolKind.FUNCTION
        else -> null
    }

    override fun isDeclaration(element: PsiElement): Boolean = element is ShFunctionDefinition

    override fun getBodyText(element: PsiElement): String? = element.text

    override fun buildSignature(element: PsiElement): String = when (element) {
        is ShFunctionDefinition -> "function ${element.name ?: "?"}"
        else -> PsiUtils.buildSignature(element)
    }
}
