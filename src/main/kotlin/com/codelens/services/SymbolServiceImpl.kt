package com.codelens.services

import com.codelens.model.SymbolInfo
import com.codelens.model.SymbolKind
import com.codelens.util.PsiUtils
import com.intellij.openapi.application.ReadAction
import com.intellij.openapi.project.DumbService
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VfsUtil
import com.intellij.psi.PsiManager
import com.intellij.psi.PsiNamedElement
import com.intellij.psi.search.FilenameIndex
import com.intellij.psi.search.GlobalSearchScope

class SymbolServiceImpl(private val project: Project) : SymbolService {

    private val adapters: List<LanguageAdapter> by lazy {
        buildList {
            // Try to load language-specific adapters (order matters: specific first)
            tryLoadAdapter { JavaLanguageAdapter() }?.let { add(it) }
            tryLoadAdapter { KotlinLanguageAdapter() }?.let { add(it) }
            tryLoadAdapter { PythonLanguageAdapter() }?.let { add(it) }
            // JavaScriptLanguageAdapter: requires JavaScript plugin (Ultimate only), see src/future/
            // Generic adapter is always available as fallback
            add(GenericLanguageAdapter())
        }
    }

    private fun tryLoadAdapter(factory: () -> LanguageAdapter): LanguageAdapter? {
        return try {
            factory()
        } catch (e: NoClassDefFoundError) {
            null // Language plugin not available
        }
    }

    private fun getAdapter(psiFile: com.intellij.psi.PsiFile): LanguageAdapter {
        return adapters.firstOrNull { it.supports(psiFile) } ?: adapters.last()
    }

    override fun getSymbolsOverview(path: String, depth: Int): List<SymbolInfo> {
        return DumbService.getInstance(project).runReadActionInSmartMode<List<SymbolInfo>> {
            val resolvedPath = resolvePath(path)
            val virtualFile = PsiUtils.resolveVirtualFile(resolvedPath)
                ?: return@runReadActionInSmartMode emptyList()

            if (virtualFile.isDirectory) {
                getDirectorySymbols(virtualFile, depth)
            } else {
                getFileSymbols(virtualFile, depth)
            }
        }
    }

    private fun getFileSymbols(
        virtualFile: com.intellij.openapi.vfs.VirtualFile,
        depth: Int
    ): List<SymbolInfo> {
        val psiFile = PsiManager.getInstance(project).findFile(virtualFile) ?: return emptyList()
        val adapter = getAdapter(psiFile)
        return adapter.extractSymbols(psiFile, depth)
    }

    private fun getDirectorySymbols(
        virtualFile: com.intellij.openapi.vfs.VirtualFile,
        depth: Int
    ): List<SymbolInfo> {
        val result = mutableListOf<SymbolInfo>()
        VfsUtil.iterateChildrenRecursively(virtualFile, { file ->
            // Skip hidden dirs, build dirs, node_modules
            !file.name.startsWith(".") &&
                file.name != "build" &&
                file.name != "out" &&
                file.name != "node_modules" &&
                file.name != "__pycache__"
        }) { file ->
            if (!file.isDirectory) {
                val psiFile = PsiManager.getInstance(project).findFile(file)
                if (psiFile != null) {
                    val adapter = getAdapter(psiFile)
                    val symbols = adapter.extractSymbols(psiFile, depth)
                    if (symbols.isNotEmpty()) {
                        result.add(
                            SymbolInfo(
                                name = PsiUtils.getRelativePath(project, file),
                                kind = SymbolKind.FILE,
                                filePath = file.path,
                                line = 0,
                                signature = "${file.name} (${symbols.size} symbols)",
                                children = symbols
                            )
                        )
                    }
                }
            }
            true
        }
        return result
    }

    override fun findSymbol(
        name: String,
        filePath: String?,
        includeBody: Boolean,
        exactMatch: Boolean
    ): List<SymbolInfo> {
        return DumbService.getInstance(project).runReadActionInSmartMode<List<SymbolInfo>> {
            if (filePath != null) {
                findSymbolInFile(name, resolvePath(filePath), includeBody, exactMatch)
            } else {
                findSymbolInProject(name, includeBody, exactMatch)
            }
        }
    }

    private fun findSymbolInFile(
        name: String,
        filePath: String,
        includeBody: Boolean,
        exactMatch: Boolean
    ): List<SymbolInfo> {
        val psiFile = PsiUtils.findPsiFile(project, filePath) ?: return emptyList()
        val adapter = getAdapter(psiFile)
        val elements = PsiUtils.findElementByName(psiFile, name, exactMatch)

        return elements.mapNotNull { element ->
            val kind = adapter.classifyElement(element) ?: SymbolKind.UNKNOWN
            SymbolInfo(
                name = element.name ?: return@mapNotNull null,
                kind = kind,
                filePath = filePath,
                line = PsiUtils.getLineNumber(element),
                column = PsiUtils.getColumnNumber(element),
                signature = adapter.buildSignature(element),
                body = if (includeBody) adapter.getBodyText(element) else null,
                documentation = PsiUtils.extractDocumentation(element)
            )
        }
    }

    private fun findSymbolInProject(
        name: String,
        includeBody: Boolean,
        exactMatch: Boolean
    ): List<SymbolInfo> {
        val scope = GlobalSearchScope.projectScope(project)
        val results = mutableListOf<SymbolInfo>()

        // Search through all project files
        // Note: For better performance, consider using GotoSymbolContributor
        val files = FilenameIndex.getAllFilesByExt(project, "java", scope) +
            FilenameIndex.getAllFilesByExt(project, "kt", scope) +
            FilenameIndex.getAllFilesByExt(project, "py", scope) +
            FilenameIndex.getAllFilesByExt(project, "js", scope) +
            FilenameIndex.getAllFilesByExt(project, "ts", scope)

        for (file in files) {
            val psiFile = PsiManager.getInstance(project).findFile(file) ?: continue
            val adapter = getAdapter(psiFile)
            val elements = PsiUtils.findElementByName(psiFile, name, exactMatch)

            for (element in elements) {
                val kind = adapter.classifyElement(element) ?: SymbolKind.UNKNOWN
                results.add(
                    SymbolInfo(
                        name = element.name ?: continue,
                        kind = kind,
                        filePath = file.path,
                        line = PsiUtils.getLineNumber(element),
                        column = PsiUtils.getColumnNumber(element),
                        signature = adapter.buildSignature(element),
                        body = if (includeBody) adapter.getBodyText(element) else null,
                        documentation = PsiUtils.extractDocumentation(element)
                    )
                )
            }

            if (results.size >= 50) break // Limit results
        }

        return results
    }

    /**
     * Resolve a potentially relative path to an absolute path.
     */
    private fun resolvePath(path: String): String {
        if (path.startsWith("/")) return path
        val basePath = project.basePath ?: return path
        return "$basePath/$path"
    }
}
