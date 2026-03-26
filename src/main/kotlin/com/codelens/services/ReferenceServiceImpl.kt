package com.codelens.services

import com.codelens.model.ReferenceInfo
import com.codelens.util.PsiUtils
import com.intellij.openapi.project.DumbService
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiNamedElement
import com.intellij.psi.search.GlobalSearchScope
import com.intellij.psi.search.searches.ReferencesSearch
import com.intellij.psi.util.PsiTreeUtil

class ReferenceServiceImpl(private val project: Project) : ReferenceService {

    override fun findReferencingSymbols(
        symbolName: String,
        filePath: String?,
        maxResults: Int
    ): List<ReferenceInfo> {
        return DumbService.getInstance(project).runReadActionInSmartMode<List<ReferenceInfo>> {
            // First, find the target symbol's PSI element
            val targetElement = resolveSymbol(symbolName, filePath) ?: return@runReadActionInSmartMode emptyList()

            val scope = GlobalSearchScope.projectScope(project)
            val references = ReferencesSearch.search(targetElement, scope)
                .findAll()
                .take(maxResults)

            references.mapNotNull { reference ->
                val refElement = reference.element
                val refFile = refElement.containingFile?.virtualFile ?: return@mapNotNull null

                // Find the containing symbol (function/class) of the reference
                val containingSymbol = PsiTreeUtil.getParentOfType(
                    refElement, PsiNamedElement::class.java
                )

                // Determine if this is a read or write reference
                val isWrite = isWriteReference(refElement)

                // Get context line
                val document = PsiUtils.getDocument(refElement.containingFile) ?: return@mapNotNull null
                val lineNum = document.getLineNumber(refElement.textOffset)
                val lineStart = document.getLineStartOffset(lineNum)
                val lineEnd = document.getLineEndOffset(lineNum)
                val lineText = document.getText(com.intellij.openapi.util.TextRange(lineStart, lineEnd)).trim()

                ReferenceInfo(
                    filePath = refFile.path,
                    line = lineNum + 1,
                    column = refElement.textOffset - lineStart + 1,
                    containingSymbol = containingSymbol?.name ?: "<file-level>",
                    context = lineText,
                    isWrite = isWrite
                )
            }
        }
    }

    /**
     * Resolve a symbol name to its PSI declaration element.
     */
    private fun resolveSymbol(name: String, filePath: String?): PsiNamedElement? {
        if (filePath != null) {
            val resolvedPath = resolvePath(filePath)
            val psiFile = PsiUtils.findPsiFile(project, resolvedPath) ?: return null
            val elements = PsiUtils.findElementByName(psiFile, name, exactMatch = true)
            return elements.firstOrNull()
        }

        // Search project-wide for the symbol
        val scope = GlobalSearchScope.projectScope(project)
        // Try Java classes first
        val javaPsiFacade = try {
            com.intellij.psi.JavaPsiFacade.getInstance(project)
        } catch (e: NoClassDefFoundError) {
            null
        }

        javaPsiFacade?.findClass(name, scope)?.let { return it }

        // Fallback: search through files
        val extensions = listOf("java", "kt", "py", "js", "ts")
        for (ext in extensions) {
            val files = try {
                com.intellij.psi.search.FilenameIndex.getAllFilesByExt(project, ext, scope)
            } catch (e: Exception) {
                continue
            }
            for (file in files) {
                val psiFile = com.intellij.psi.PsiManager.getInstance(project).findFile(file) ?: continue
                val elements = PsiUtils.findElementByName(psiFile, name, exactMatch = true)
                elements.firstOrNull()?.let { return it }
            }
        }

        return null
    }

    /**
     * Check if a reference is a write (assignment) reference.
     */
    private fun isWriteReference(element: com.intellij.psi.PsiElement): Boolean {
        val parent = element.parent ?: return false
        val parentText = parent.text
        val elementOffset = element.startOffsetInParent

        // Simple heuristic: check if element is on the left side of an assignment
        val afterElement = parentText.substring(elementOffset + element.textLength).trimStart()
        return afterElement.startsWith("=") && !afterElement.startsWith("==")
    }

    private fun resolvePath(path: String): String {
        if (path.startsWith("/")) return path
        val basePath = project.basePath ?: return path
        return "$basePath/$path"
    }
}
