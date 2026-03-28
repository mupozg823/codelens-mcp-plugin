package com.codelens.tools

import com.codelens.util.PsiUtils
import com.intellij.ide.hierarchy.HierarchyBrowserManager
import com.intellij.ide.hierarchy.call.CallHierarchyNodeDescriptor
import com.intellij.ide.hierarchy.call.CalleeMethodsTreeStructure
import com.intellij.ide.hierarchy.call.CallerMethodsTreeStructure
import com.intellij.openapi.project.DumbService
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiManager
import com.intellij.psi.PsiMember
import com.intellij.psi.PsiNamedElement
import com.intellij.psi.search.FilenameIndex
import com.intellij.psi.search.GlobalSearchScope

/**
 * Call Hierarchy tool using IntelliJ's CallHierarchy API.
 * Finds callers (who calls this?) and callees (what does this call?) of a method/function.
 */
class CallHierarchyTool : BaseMcpTool() {
    override val toolName = "get_call_hierarchy"
    override val description = """
        Get the call hierarchy for a function or method.
        Shows who calls this function (callers) and/or what this function calls (callees).
        Requires JetBrains backend (IntelliJ PSI).
    """.trimIndent()

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "symbol_name" to mapOf(
                "type" to "string",
                "description" to "Name of the function or method"
            ),
            "relative_path" to mapOf(
                "type" to "string",
                "description" to "Relative path to the file containing the symbol"
            ),
            "direction" to mapOf(
                "type" to "string",
                "description" to "Which direction: 'callers' (who calls this?), 'callees' (what does this call?), or 'both'",
                "enum" to listOf("callers", "callees", "both"),
                "default" to "callers"
            ),
            "depth" to mapOf(
                "type" to "integer",
                "description" to "Maximum depth to traverse (1 = direct only)",
                "default" to 1
            ),
            "max_results" to mapOf(
                "type" to "integer",
                "description" to "Maximum number of results",
                "default" to 50
            )
        ),
        "required" to listOf("symbol_name")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        return try {
            val symbolName = requireString(args, "symbol_name")
            val filePath = optionalString(args, "relative_path")
            val direction = optionalString(args, "direction") ?: "callers"
            val depth = optionalInt(args, "depth", 1)
            val maxResults = optionalInt(args, "max_results", 50)

            val result = DumbService.getInstance(project).runReadActionInSmartMode<Map<String, Any?>> {
                val element = findTargetElement(project, symbolName, filePath)
                    ?: return@runReadActionInSmartMode mapOf("error" to "Symbol '$symbolName' not found")

                val callers = if (direction == "callers" || direction == "both") {
                    findCallers(project, element, depth, maxResults)
                } else emptyList()

                val callees = if (direction == "callees" || direction == "both") {
                    findCallees(project, element, depth, maxResults)
                } else emptyList()

                mapOf(
                    "symbol" to symbolName,
                    "file" to (filePath ?: element.containingFile?.virtualFile?.path),
                    "line" to PsiUtils.getLineNumber(element),
                    "callers" to callers,
                    "callees" to callees,
                    "callers_count" to callers.size,
                    "callees_count" to callees.size
                )
            }

            successResponse(result)
        } catch (e: Exception) {
            errorResponse("Call hierarchy failed: ${e.message}")
        }
    }

    private fun findTargetElement(project: Project, name: String, filePath: String?): PsiElement? {
        if (filePath != null) {
            val resolvedPath = if (filePath.startsWith("/")) filePath else "${project.basePath}/$filePath"
            val psiFile = PsiUtils.findPsiFile(project, resolvedPath) ?: return null
            val elements = PsiUtils.findElementByName(psiFile, name, true)
            return elements.firstOrNull()
        }

        // Search across common code files
        val scope = GlobalSearchScope.projectScope(project)
        val extensions = listOf("java", "kt", "py", "js", "ts", "go")
        for (ext in extensions) {
            for (file in FilenameIndex.getAllFilesByExt(project, ext, scope)) {
                val psiFile = PsiManager.getInstance(project).findFile(file) ?: continue
                val elements = PsiUtils.findElementByName(psiFile, name, true)
                if (elements.isNotEmpty()) return elements.first()
            }
        }
        return null
    }

    private fun findCallers(
        project: Project,
        element: PsiElement,
        depth: Int,
        maxResults: Int
    ): List<Map<String, Any?>> {
        val member = element as? PsiMember ?: return emptyList()
        return try {
            val scope = HierarchyBrowserManager.getInstance(project).state?.SCOPE ?: ""
            val treeStructure = CallerMethodsTreeStructure(project, member, scope)
            collectHierarchyNodes(treeStructure, treeStructure.rootElement, depth, maxResults, mutableListOf())
        } catch (e: Exception) {
            emptyList()
        }
    }

    private fun findCallees(
        project: Project,
        element: PsiElement,
        depth: Int,
        maxResults: Int
    ): List<Map<String, Any?>> {
        val member = element as? PsiMember ?: return emptyList()
        return try {
            val scope = HierarchyBrowserManager.getInstance(project).state?.SCOPE ?: ""
            val treeStructure = CalleeMethodsTreeStructure(project, member, scope)
            collectHierarchyNodes(treeStructure, treeStructure.rootElement, depth, maxResults, mutableListOf())
        } catch (e: Exception) {
            emptyList()
        }
    }

    private fun collectHierarchyNodes(
        treeStructure: com.intellij.ide.util.treeView.AbstractTreeStructure,
        node: Any,
        maxDepth: Int,
        maxResults: Int,
        results: MutableList<Map<String, Any?>>,
        currentDepth: Int = 0
    ): List<Map<String, Any?>> {
        if (currentDepth > maxDepth || results.size >= maxResults) return results

        val children = treeStructure.getChildElements(node)
        for (child in children) {
            if (results.size >= maxResults) break

            if (child is CallHierarchyNodeDescriptor) {
                val psiElement = child.psiElement ?: continue
                val containingFile = psiElement.containingFile?.virtualFile?.path
                val name = (psiElement as? PsiNamedElement)?.name ?: psiElement.text.take(50)

                results.add(
                    mapOf(
                        "name" to name,
                        "file" to containingFile,
                        "line" to PsiUtils.getLineNumber(psiElement),
                        "depth" to currentDepth
                    )
                )

                if (currentDepth < maxDepth) {
                    collectHierarchyNodes(treeStructure, child, maxDepth, maxResults, results, currentDepth + 1)
                }
            }
        }
        return results
    }
}
