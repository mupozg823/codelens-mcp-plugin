package com.codelens.util

import com.intellij.openapi.editor.Document
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.openapi.vfs.VirtualFileManager
import com.intellij.openapi.roots.ProjectRootManager
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.PsiManager
import com.intellij.psi.PsiNamedElement
import com.intellij.psi.util.PsiTreeUtil

/**
 * Utility functions for PSI tree navigation and manipulation.
 */
object PsiUtils {

    /**
     * Resolve a file path to a PsiFile.
     */
    fun findPsiFile(project: Project, filePath: String): PsiFile? {
        val virtualFile = resolveVirtualFile(filePath) ?: return null
        return PsiManager.getInstance(project).findFile(virtualFile)
    }

    /**
     * Resolve a file path to a VirtualFile.
     * Tries LocalFileSystem first, then VirtualFileManager for non-local VFS (e.g., test fixtures).
     */
    fun resolveVirtualFile(filePath: String): VirtualFile? {
        return LocalFileSystem.getInstance().findFileByPath(filePath)
            ?: VirtualFileManager.getInstance().findFileByUrl("file://$filePath")
            ?: VirtualFileManager.getInstance().findFileByUrl("temp://$filePath")
    }

    /**
     * Get the line number (1-based) for a PSI element.
     */
    fun getLineNumber(element: PsiElement): Int {
        val file = element.containingFile ?: return -1
        val document = getDocument(file) ?: return -1
        val offset = element.textOffset
        return document.getLineNumber(offset) + 1 // Convert 0-based to 1-based
    }

    /**
     * Get the column number (1-based) for a PSI element.
     */
    fun getColumnNumber(element: PsiElement): Int {
        val file = element.containingFile ?: return -1
        val document = getDocument(file) ?: return -1
        val offset = element.textOffset
        val lineNumber = document.getLineNumber(offset)
        val lineStartOffset = document.getLineStartOffset(lineNumber)
        return offset - lineStartOffset + 1 // 1-based
    }

    /**
     * Get the Document for a PsiFile.
     */
    fun getDocument(psiFile: PsiFile): Document? {
        return PsiDocumentManager.getInstance(psiFile.project).getDocument(psiFile)
    }

    /**
     * Get the relative path of a file within the project.
     */
    fun getRelativePath(project: Project, virtualFile: VirtualFile): String {
        val matchingRoot = getProjectRoots(project)
            .filter { virtualFile.path.startsWith(it.path) }
            .maxByOrNull { it.path.length }
        if (matchingRoot != null) {
            return virtualFile.path.removePrefix(matchingRoot.path).removePrefix("/")
        }
        val projectRoot = project.baseDir?.path ?: project.basePath ?: return virtualFile.path
        return virtualFile.path.removePrefix(projectRoot).removePrefix("/")
    }

    fun findProjectFile(project: Project, relativePath: String): VirtualFile? {
        if (relativePath.startsWith("/")) return resolveVirtualFile(relativePath)
        for (root in getProjectRoots(project)) {
            root.findFileByRelativePath(relativePath)?.let { return it }
        }
        project.baseDir?.findFileByRelativePath(relativePath)?.let { return it }
        val basePath = project.basePath ?: return resolveVirtualFile(relativePath)
        return resolveVirtualFile("$basePath/$relativePath")
    }

    fun getProjectRoots(project: Project): List<VirtualFile> {
        val contentRoots = ProjectRootManager.getInstance(project).contentRoots.toList()
        if (contentRoots.isNotEmpty()) return contentRoots
        val fallbacks = mutableListOf<VirtualFile>()
        project.baseDir?.let { fallbacks.add(it) }
        project.basePath?.let { resolveVirtualFile(it)?.let(fallbacks::add) }
        return fallbacks.distinctBy { it.path }
    }

    /**
     * Build a human-readable signature for a PSI element.
     */
    fun buildSignature(element: PsiElement): String {
        val text = element.text
        // Take only the first line, truncated
        val firstLine = text.lines().firstOrNull()?.take(200) ?: ""
        // Remove body content (everything after { or =)
        return firstLine
            .replace(Regex("\\{.*"), "{ ... }")
            .replace(Regex("=\\s*\\S.*"), "= ...")
            .trim()
    }

    /**
     * Extract the documentation comment for a PSI element.
     */
    fun extractDocumentation(element: PsiElement): String? {
        // Look for doc comment immediately before the element
        var prev = element.prevSibling
        while (prev != null) {
            val text = prev.text.trim()
            if (text.startsWith("/**") || text.startsWith("///") || text.startsWith("##")) {
                return text
            }
            if (text.isNotEmpty() && !text.startsWith("//") && !text.startsWith("#")) {
                break
            }
            prev = prev.prevSibling
        }
        return null
    }

    /**
     * Find all named elements in a PSI file up to a given depth.
     */
    fun findNamedElements(
        root: PsiElement,
        maxDepth: Int = 1,
        currentDepth: Int = 0
    ): List<PsiNamedElement> {
        if (currentDepth > maxDepth) return emptyList()

        val result = mutableListOf<PsiNamedElement>()
        for (child in root.children) {
            if (child is PsiNamedElement && child.name != null) {
                result.add(child)
                if (currentDepth < maxDepth) {
                    result.addAll(findNamedElements(child, maxDepth, currentDepth + 1))
                }
            } else {
                // Recurse into non-named containers (e.g., package statements, file-level)
                result.addAll(findNamedElements(child, maxDepth, currentDepth))
            }
        }
        return result
    }

    /**
     * Find a named element by name within a given scope.
     * @param declarationsOnly If true, filter out local variables, parameters, and references — only return top-level declarations.
     */
    fun findElementByName(
        root: PsiElement,
        name: String,
        exactMatch: Boolean = true,
        declarationsOnly: Boolean = false
    ): List<PsiNamedElement> {
        val allNamed = PsiTreeUtil.findChildrenOfType(root, PsiNamedElement::class.java)
        return allNamed
            .filter { element ->
                val elementName = element.name ?: return@filter false
                val nameMatch = if (exactMatch) elementName == name
                    else elementName.contains(name, ignoreCase = true)
                if (!nameMatch) return@filter false
                if (declarationsOnly) isDeclaration(element) else true
            }
            .sortedByDescending { isDeclaration(it) }
    }

    /**
     * Check if a PSI element is a top-level declaration (class, method, function, field, property, etc.)
     * rather than a local variable, parameter, or reference.
     */
    private fun isDeclaration(element: PsiElement): Boolean {
        val className = element.javaClass.simpleName
        return className.contains("Class") ||
            className.contains("Method") ||
            className.contains("Function") ||
            className.contains("Field") ||
            className.contains("Property") ||
            className.contains("Object") ||
            className.contains("TypeAlias") ||
            className.contains("Enum") ||
            className.contains("Interface")
    }
}
