package com.codelens.serena

import com.codelens.model.SymbolKind
import com.codelens.util.PsiUtils
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.application.ReadAction
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VfsUtil
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.openapi.vfs.VirtualFileFilter
import com.intellij.openapi.vfs.VirtualFileManager
import com.intellij.openapi.vfs.newvfs.BulkFileListener
import com.intellij.openapi.vfs.newvfs.events.VFileEvent
import java.util.concurrent.ConcurrentHashMap
import com.intellij.psi.JavaPsiFacade
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.PsiManager
import com.intellij.psi.PsiNamedElement
import com.intellij.psi.search.GlobalSearchScope
import com.intellij.psi.search.searches.ReferencesSearch
import com.intellij.psi.search.searches.ClassInheritorsSearch
import com.intellij.psi.util.PsiTreeUtil
import org.jetbrains.kotlin.asJava.classes.KtLightClass
import org.jetbrains.kotlin.psi.KtClass
import java.nio.file.Path

internal class SerenaCompatSymbols(private val project: Project) {

    data class SymbolRecord(
        val element: PsiNamedElement,
        val namePath: String,
        val relativePath: String,
        val kind: SymbolKind,
        val children: List<SymbolRecord> = emptyList()
    )

    private val symbolCache = ConcurrentHashMap<String, List<SymbolRecord>>()

    private val dirFilter = VirtualFileFilter { file ->
        if (!file.isDirectory) true
        else file.name !in EXCLUDED_DIRS
    }

    init {
        project.messageBus.connect().subscribe(VirtualFileManager.VFS_CHANGES, object : BulkFileListener {
            override fun after(events: List<VFileEvent>) {
                for (event in events) {
                    val path = event.file?.let { projectRelativePath(it.path) } ?: continue
                    symbolCache.keys.removeIf { it.startsWith(path) || path.startsWith(it) }
                }
            }
        })
    }

    fun projectRelativePath(path: String): String {
        if (path.startsWith("/")) {
            val basePath = project.basePath ?: return path
            return Path.of(basePath).relativize(Path.of(path)).toString().replace('\\', '/')
        }
        return path.replace('\\', '/')
    }

    fun findSymbols(
        namePathPattern: String,
        relativePath: String? = null,
        includeBody: Boolean = false,
        includeQuickInfo: Boolean = false,
        includeDocumentation: Boolean = false,
        includeNumUsages: Boolean = false,
        depth: Int = 0,
        includeLocation: Boolean = false
    ): List<Map<String, Any?>> {
        return ReadAction.compute<List<Map<String, Any?>>, Exception> {
            scanDeclarations(relativePath, depth).asSequence()
                .filter { matchesNamePathPattern(namePathPattern, it.namePath) }
                .map {
                    toSymbolDto(
                        record = it,
                        includeBody = includeBody,
                        includeQuickInfo = includeQuickInfo,
                        includeDocumentation = includeDocumentation,
                        includeNumUsages = includeNumUsages,
                        includeLocation = includeLocation
                    )
                }
                .toList()
        }
    }

    fun getSymbolsOverview(relativePath: String, depth: Int, includeFileDocumentation: Boolean): Map<String, Any?> {
        return ReadAction.compute<Map<String, Any?>, Exception> {
            val normalizedPath = projectRelativePath(relativePath)
            val file = resolveProjectFile(normalizedPath) ?: return@compute mapOf("symbols" to emptyList<Map<String, Any?>>())
            val symbols = if (file.isDirectory) {
                getDirectoryOverview(file, depth)
            } else {
                val psiFile = PsiManager.getInstance(project).findFile(file)
                collectFileDeclarations(psiFile, depth).map {
                    toSymbolDto(
                        record = it,
                        includeBody = false,
                        includeQuickInfo = true,
                        includeDocumentation = true,
                        includeNumUsages = false,
                        includeLocation = true
                    )
                }
            }
            buildMap {
                put("symbols", symbols)
                if (includeFileDocumentation && !file.isDirectory) {
                    val psiFile = PsiManager.getInstance(project).findFile(file)
                    psiFile?.firstChild?.let(PsiUtils::extractDocumentation)?.let { put("documentation", "<pre>$it</pre>") }
                }
            }
        }
    }

    fun findReferences(namePath: String, relativePath: String, includeQuickInfo: Boolean): List<Map<String, Any?>> {
        return ReadAction.compute<List<Map<String, Any?>>, Exception> {
            val record = findUniqueDeclaration(namePath, relativePath)
                ?: throw IllegalArgumentException("No symbol with name path '$namePath' found in '$relativePath'")

            val references = ReferencesSearch.search(record.element, GlobalSearchScope.projectScope(project))
                .findAll()

            references.mapNotNull { ref ->
                val containing = PsiTreeUtil.getParentOfType(ref.element, PsiNamedElement::class.java)
                    ?: return@mapNotNull null
                toReferenceDto(containing, includeQuickInfo)
            }.distinctBy { "${it["relativePath"]}:${it["namePath"]}:${it["type"]}" }
        }
    }

    fun findReferencingCodeSnippets(
        namePath: String,
        relativePath: String,
        contextLinesBefore: Int = 2,
        contextLinesAfter: Int = 2
    ): List<Map<String, Any?>> {
        return ReadAction.compute<List<Map<String, Any?>>, Exception> {
            val record = findUniqueDeclaration(namePath, relativePath)
                ?: throw IllegalArgumentException("No symbol with name path '$namePath' found in '$relativePath'")

            val references = ReferencesSearch.search(record.element, GlobalSearchScope.projectScope(project))
                .findAll()

            references.mapNotNull { ref ->
                val element = ref.element
                val psiFile = element.containingFile ?: return@mapNotNull null
                val document = PsiDocumentManager.getInstance(project).getDocument(psiFile) ?: return@mapNotNull null
                val lineNumber = document.getLineNumber(element.textOffset)
                val startLine = maxOf(0, lineNumber - contextLinesBefore)
                val endLine = minOf(document.lineCount - 1, lineNumber + contextLinesAfter)
                val startOffset = document.getLineStartOffset(startLine)
                val endOffset = document.getLineEndOffset(endLine)
                val snippet = document.getText(com.intellij.openapi.util.TextRange(startOffset, endOffset))
                val filePath = psiFile.virtualFile?.let { projectRelativePath(it.path) } ?: return@mapNotNull null

                mapOf(
                    "relativePath" to filePath,
                    "line" to lineNumber + 1,
                    "startLine" to startLine + 1,
                    "endLine" to endLine + 1,
                    "snippet" to snippet
                )
            }.distinctBy { "${it["relativePath"]}:${it["line"]}" }
        }
    }

    fun resolveNamedElement(namePath: String, relativePath: String): PsiNamedElement? {
        return ReadAction.compute<PsiNamedElement?, Exception> {
            findUniqueDeclaration(namePath, relativePath)?.element
        }
    }

    fun replaceSymbolBody(namePath: String, relativePath: String, body: String) {
        val element = ReadAction.compute<PsiNamedElement, Exception> {
            findUniqueDeclaration(namePath, relativePath)?.element
                ?: throw IllegalArgumentException("No symbol '$namePath' found in '$relativePath'")
        }
        ApplicationManager.getApplication().invokeAndWait {
            WriteCommandAction.runWriteCommandAction(project) {
                val document = PsiDocumentManager.getInstance(project).getDocument(element.containingFile)
                    ?: throw IllegalStateException("Cannot obtain document for '${element.containingFile.name}'")
                val range = element.textRange
                document.replaceString(range.startOffset, range.endOffset, body)
                PsiDocumentManager.getInstance(project).commitDocument(document)
            }
        }
    }

    fun insertAfterSymbol(namePath: String, relativePath: String, body: String) {
        val element = ReadAction.compute<PsiNamedElement, Exception> {
            findUniqueDeclaration(namePath, relativePath)?.element
                ?: throw IllegalArgumentException("No symbol '$namePath' found in '$relativePath'")
        }
        ApplicationManager.getApplication().invokeAndWait {
            WriteCommandAction.runWriteCommandAction(project) {
                val document = PsiDocumentManager.getInstance(project).getDocument(element.containingFile)
                    ?: throw IllegalStateException("Cannot obtain document for '${element.containingFile.name}'")
                document.insertString(element.textRange.endOffset, "\n$body")
                PsiDocumentManager.getInstance(project).commitDocument(document)
            }
        }
    }

    fun insertBeforeSymbol(namePath: String, relativePath: String, body: String) {
        val element = ReadAction.compute<PsiNamedElement, Exception> {
            findUniqueDeclaration(namePath, relativePath)?.element
                ?: throw IllegalArgumentException("No symbol '$namePath' found in '$relativePath'")
        }
        ApplicationManager.getApplication().invokeAndWait {
            WriteCommandAction.runWriteCommandAction(project) {
                val document = PsiDocumentManager.getInstance(project).getDocument(element.containingFile)
                    ?: throw IllegalStateException("Cannot obtain document for '${element.containingFile.name}'")
                document.insertString(element.textRange.startOffset, "$body\n")
                PsiDocumentManager.getInstance(project).commitDocument(document)
            }
        }
    }

    fun getSupertypes(namePath: String, relativePath: String, depth: Int?, limitChildren: Int?): Map<String, Any?> {
        return ReadAction.compute<Map<String, Any?>, Exception> {
            val psiClass = resolvePsiClass(namePath, relativePath)
                ?: throw IllegalArgumentException("No class symbol '$namePath' found in '$relativePath'")
            val supers = psiClass.supers.toList()
            val hierarchy = applyChildLimit(supers, limitChildren)
                .mapNotNull { buildTypeHierarchyNode(it, normalizeHierarchyDepth(depth), limitChildren, Direction.SUPER, mutableSetOf()) }
            hierarchyResponse(hierarchy, supers.size, limitChildren)
        }
    }

    fun getSubtypes(namePath: String, relativePath: String, depth: Int?, limitChildren: Int?): Map<String, Any?> {
        return ReadAction.compute<Map<String, Any?>, Exception> {
            val psiClass = resolvePsiClass(namePath, relativePath)
                ?: throw IllegalArgumentException("No class symbol '$namePath' found in '$relativePath'")
            val inheritors = ClassInheritorsSearch.search(psiClass, GlobalSearchScope.projectScope(project), true).findAll()
            val hierarchy = applyChildLimit(inheritors, limitChildren)
                .mapNotNull { buildTypeHierarchyNode(it, normalizeHierarchyDepth(depth), limitChildren, Direction.SUB, mutableSetOf()) }
            hierarchyResponse(hierarchy, inheritors.size, limitChildren)
        }
    }

    private fun getDirectoryOverview(directory: VirtualFile, depth: Int): List<Map<String, Any?>> {
        val result = mutableListOf<Map<String, Any?>>()
        VfsUtil.iterateChildrenRecursively(directory, dirFilter) { file ->
            if (!file.isDirectory) {
                val psiFile = PsiManager.getInstance(project).findFile(file)
                val symbols = collectFileDeclarations(psiFile, depth)
                if (symbols.isNotEmpty()) {
                    result += symbols.map {
                        toSymbolDto(
                            record = it,
                            includeBody = false,
                            includeQuickInfo = true,
                            includeDocumentation = true,
                            includeNumUsages = false,
                            includeLocation = true
                        )
                    }
                }
            }
            true
        }
        return result
    }

    private fun scanDeclarations(relativePath: String?, depth: Int): List<SymbolRecord> {
        return when {
            relativePath == null -> collectProjectDeclarations(depth)
            else -> {
                val file = resolveProjectFile(relativePath) ?: return emptyList()
                if (file.isDirectory) {
                    collectDirectoryDeclarations(file, depth)
                } else {
                    collectFileDeclarations(PsiManager.getInstance(project).findFile(file), depth)
                }
            }
        }
    }

    private fun collectProjectDeclarations(depth: Int): List<SymbolRecord> {
        val basePath = project.basePath ?: return emptyList()
        val root = PsiUtils.resolveVirtualFile(basePath) ?: return emptyList()
        return collectDirectoryDeclarations(root, depth)
    }

    private fun collectDirectoryDeclarations(directory: VirtualFile, depth: Int): List<SymbolRecord> {
        val result = mutableListOf<SymbolRecord>()
        VfsUtil.iterateChildrenRecursively(directory, dirFilter) { file ->
            if (!file.isDirectory) {
                val psiFile = PsiManager.getInstance(project).findFile(file)
                result += collectFileDeclarations(psiFile, depth)
            }
            true
        }
        return result
    }

    private fun collectFileDeclarations(psiFile: PsiFile?, depth: Int): List<SymbolRecord> {
        if (psiFile == null) return emptyList()
        val relativePath = psiFile.virtualFile?.let { PsiUtils.getRelativePath(project, it) } ?: return emptyList()
        return psiFile.children.flatMap { child ->
            collectDeclarationRecords(child, relativePath, depth, 0, emptyList())
        }
    }

    private fun collectDeclarationRecords(
        element: PsiElement,
        relativePath: String,
        maxDepth: Int,
        currentDepth: Int,
        parentPath: List<String>
    ): List<SymbolRecord> {
        val named = element as? PsiNamedElement
        val name = named?.name
        val kind = if (named != null) classifyDeclaration(element) else null

        if (named != null && name != null && kind != null) {
            val currentNamePath = parentPath + name
            val childRecords = if (currentDepth < maxDepth) {
                element.children.flatMap { child ->
                    collectDeclarationRecords(child, relativePath, maxDepth, currentDepth + 1, currentNamePath)
                }
            } else {
                emptyList()
            }
            return listOf(
                SymbolRecord(
                    element = named,
                    namePath = currentNamePath.joinToString("/"),
                    relativePath = relativePath,
                    kind = kind,
                    children = childRecords
                )
            )
        }

        return element.children.flatMap { child ->
            collectDeclarationRecords(child, relativePath, maxDepth, currentDepth, parentPath)
        }
    }

    private fun classifyDeclaration(element: PsiElement): SymbolKind? {
        val simpleName = element.javaClass.simpleName
        return when {
            simpleName.contains("Class") -> SymbolKind.CLASS
            simpleName.contains("Interface") -> SymbolKind.INTERFACE
            simpleName.contains("Enum") -> SymbolKind.ENUM
            simpleName.contains("Annotation") -> SymbolKind.ANNOTATION
            simpleName.contains("Constructor") -> SymbolKind.CONSTRUCTOR
            simpleName.contains("Method") -> SymbolKind.METHOD
            simpleName.contains("Function") -> SymbolKind.FUNCTION
            simpleName.contains("Property") -> SymbolKind.PROPERTY
            simpleName.contains("Field") -> SymbolKind.FIELD
            simpleName.contains("Object") -> SymbolKind.OBJECT
            simpleName.contains("TypeAlias") -> SymbolKind.TYPE_ALIAS
            else -> null
        }
    }

    private fun matchesNamePathPattern(pattern: String, namePath: String): Boolean {
        val normalizedPattern = pattern.removePrefix("/")
        return when {
            pattern.startsWith("/") -> namePath == normalizedPattern
            normalizedPattern.contains("/") -> namePath == normalizedPattern || namePath.endsWith("/$normalizedPattern")
            else -> namePath.substringAfterLast("/") == normalizedPattern
        }
    }

    private fun toSymbolDto(
        record: SymbolRecord,
        includeBody: Boolean,
        includeQuickInfo: Boolean,
        includeDocumentation: Boolean,
        includeNumUsages: Boolean,
        includeLocation: Boolean
    ): Map<String, Any?> {
        val children = record.children.map {
            toSymbolDto(
                record = it,
                includeBody = includeBody,
                includeQuickInfo = includeQuickInfo,
                includeDocumentation = includeDocumentation,
                includeNumUsages = includeNumUsages,
                includeLocation = includeLocation
            )
        }

        return buildMap {
            put("namePath", record.namePath)
            put("relativePath", record.relativePath)
            put("type", record.kind.displayName)
            if (includeBody) {
                put("body", record.element.text)
            }
            if (includeQuickInfo) {
                put("quickInfo", record.element.text.lines().firstOrNull()?.take(300) ?: record.namePath)
            }
            if (includeDocumentation) {
                PsiUtils.extractDocumentation(record.element)?.let { put("documentation", "<pre>$it</pre>") }
            }
            if (includeLocation) {
                buildTextRange(record.element)?.let { put("textRange", it) }
            }
            if (includeNumUsages) {
                put("numUsages", ReferencesSearch.search(record.element, GlobalSearchScope.projectScope(project)).findAll().size)
            }
            if (children.isNotEmpty()) {
                put("children", children)
            }
        }
    }

    private fun toReferenceDto(element: PsiNamedElement, includeQuickInfo: Boolean): Map<String, Any?>? {
        val file = element.containingFile?.virtualFile ?: return null
        val relativePath = PsiUtils.getRelativePath(project, file)
        val namePath = computeNamePath(element) ?: return null
        val kind = classifyDeclaration(element as PsiElement) ?: SymbolKind.UNKNOWN
        return buildMap {
            put("namePath", namePath)
            put("relativePath", relativePath)
            put("type", kind.displayName)
            buildTextRange(element as PsiElement)?.let { put("textRange", it) }
            if (includeQuickInfo) {
                put("quickInfo", element.text.lines().firstOrNull()?.take(300) ?: namePath)
            }
        }
    }

    private fun buildTextRange(element: PsiElement): Map<String, Any?>? {
        val file = element.containingFile ?: return null
        val document = PsiDocumentManager.getInstance(project).getDocument(file) ?: return null
        val range = element.textRange
        fun pos(offset: Int): Map<String, Int> {
            val line = document.getLineNumber(offset)
            val lineStart = document.getLineStartOffset(line)
            return mapOf("line" to (line + 1), "col" to (offset - lineStart + 1))
        }
        return mapOf(
            "startPos" to pos(range.startOffset),
            "endPos" to pos(range.endOffset)
        )
    }

    private fun findUniqueDeclaration(namePath: String, relativePath: String): SymbolRecord? {
        val targetDepth = namePath.count { it == '/' }
        val cacheKey = "$relativePath:$targetDepth"
        val declarations = symbolCache.getOrPut(cacheKey) {
            collectFileDeclarations(resolvePsiFile(relativePath), targetDepth)
        }
        return declarations.firstOrNull { it.namePath == namePath }
    }

    private fun resolvePsiClass(namePath: String, relativePath: String): com.intellij.psi.PsiClass? {
        val record = findUniqueDeclaration(namePath, relativePath) ?: return null
        val element = record.element
        return when (element) {
            is com.intellij.psi.PsiClass -> element
            is KtClass -> element.fqName?.asString()?.let {
                JavaPsiFacade.getInstance(project).findClass(it, GlobalSearchScope.projectScope(project))
            }
            else -> null
        }
    }

    private fun buildTypeHierarchyNode(
        psiClass: com.intellij.psi.PsiClass,
        remainingDepth: Int?,
        limitChildren: Int?,
        direction: Direction,
        visited: MutableSet<String>
    ): Map<String, Any?>? {
        val qualifiedName = psiClass.qualifiedName ?: psiClass.name ?: return null
        if (!visited.add(qualifiedName)) {
            return null
        }

        val relativePath = psiClass.containingFile?.virtualFile?.let { PsiUtils.getRelativePath(project, it) } ?: return null
        val symbol = buildMap<String, Any?> {
            put("namePath", computeNamePath(psiClass) ?: (psiClass.name ?: qualifiedName))
            put("relativePath", relativePath)
            put("type", if (psiClass.isInterface) "interface" else "class")
            buildTextRange(psiClass)?.let { put("textRange", it) }
            put("quickInfo", psiClass.name ?: qualifiedName)
        }

        val nextDepth = remainingDepth?.let { it - 1 }
        val children = if (remainingDepth == null || remainingDepth > 1) {
            when (direction) {
                Direction.SUPER -> applyChildLimit(psiClass.supers.toList(), limitChildren)
                    .mapNotNull { buildTypeHierarchyNode(it, nextDepth, limitChildren, direction, visited) }
                Direction.SUB -> applyChildLimit(
                    ClassInheritorsSearch.search(psiClass, GlobalSearchScope.projectScope(project), true).findAll(),
                    limitChildren
                ).mapNotNull { buildTypeHierarchyNode(it, nextDepth, limitChildren, direction, visited) }
            }
        } else {
            emptyList()
        }

        return buildMap {
            put("symbol", symbol)
            if (children.isNotEmpty()) {
                put("children", children)
            }
        }
    }

    private fun computeNamePath(element: PsiNamedElement): String? {
        val parts = mutableListOf<String>()
        var current: PsiElement? = element as PsiElement
        while (current != null) {
            if (current is PsiNamedElement) {
                val name = current.name
                val kind = classifyDeclaration(current)
                if (name != null && kind != null) {
                    parts += name
                }
            }
            current = current.parent
        }
        return parts.asReversed().joinToString("/").ifBlank { null }
    }

    private fun resolveProjectFile(relativePath: String): com.intellij.openapi.vfs.VirtualFile? {
        val basePath = project.basePath ?: return null
        return PsiUtils.resolveVirtualFile("$basePath/${relativePath.removePrefix("/")}")
    }

    private fun resolvePsiFile(relativePath: String): PsiFile? {
        val file = resolveProjectFile(relativePath) ?: return null
        return PsiManager.getInstance(project).findFile(file)
    }

    private fun normalizeHierarchyDepth(depth: Int?): Int? {
        return when {
            depth == null -> null
            depth <= 0 -> null
            else -> depth
        }
    }

    private fun <T> applyChildLimit(items: Collection<T>, limitChildren: Int?): List<T> {
        return when {
            limitChildren == null || limitChildren <= 0 -> items.toList()
            else -> items.take(limitChildren)
        }
    }

    private fun hierarchyResponse(
        hierarchy: List<Map<String, Any?>>,
        totalChildren: Int,
        limitChildren: Int?
    ): Map<String, Any?> {
        return buildMap {
            put("hierarchy", hierarchy)
            if (limitChildren != null && limitChildren > 0 && totalChildren > limitChildren) {
                put("numLevelsNotIncluded", totalChildren - limitChildren)
            }
        }
    }

    private enum class Direction {
        SUPER,
        SUB
    }

    companion object {
        private val EXCLUDED_DIRS = setOf(
            ".git", ".idea", ".gradle", ".serena",
            "build", "out", "dist", "target",
            "node_modules", "__pycache__", ".venv", "venv"
        )
    }
}
