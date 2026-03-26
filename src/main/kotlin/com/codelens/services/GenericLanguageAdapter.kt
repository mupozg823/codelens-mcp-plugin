package com.codelens.services

import com.codelens.model.SymbolInfo
import com.codelens.model.SymbolKind
import com.codelens.util.PsiUtils
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.PsiNamedElement
import com.intellij.psi.util.PsiTreeUtil

/**
 * Fallback language adapter using generic PSI navigation.
 * Works for any language but with less precision than specialized adapters.
 */
class GenericLanguageAdapter : LanguageAdapter {

    override val languageId: String = "generic"

    override fun supports(psiFile: PsiFile): Boolean = true // Fallback for all

    override fun extractSymbols(psiFile: PsiFile, depth: Int): List<SymbolInfo> {
        val namedElements = PsiUtils.findNamedElements(psiFile, maxDepth = depth)
        return namedElements.mapNotNull { element ->
            val name = element.name ?: return@mapNotNull null
            val kind = classifyElement(element) ?: SymbolKind.UNKNOWN
            val file = element.containingFile?.virtualFile?.path ?: ""

            SymbolInfo(
                name = name,
                kind = kind,
                filePath = file,
                line = PsiUtils.getLineNumber(element),
                column = PsiUtils.getColumnNumber(element),
                signature = buildSignature(element),
                documentation = PsiUtils.extractDocumentation(element)
            )
        }
    }

    override fun classifyElement(element: PsiElement): SymbolKind? {
        val className = element.javaClass.simpleName
        return SymbolKind.fromPsiElement(className)
    }

    override fun isDeclaration(element: PsiElement): Boolean {
        return element is PsiNamedElement
    }

    override fun getBodyText(element: PsiElement): String? {
        return element.text
    }

    override fun buildSignature(element: PsiElement): String {
        return PsiUtils.buildSignature(element)
    }
}
