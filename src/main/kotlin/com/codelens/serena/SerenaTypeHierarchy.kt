package com.codelens.serena

import com.codelens.util.PsiUtils
import com.intellij.openapi.application.ReadAction
import com.intellij.openapi.project.Project
import com.intellij.psi.JavaPsiFacade
import com.intellij.psi.PsiNamedElement
import com.intellij.psi.search.GlobalSearchScope
import com.intellij.psi.search.searches.ClassInheritorsSearch
import org.jetbrains.kotlin.psi.KtClass

internal class SerenaTypeHierarchy(
    private val project: Project,
    private val reader: SerenaSymbolReader
) {

    enum class Direction {
        SUPER,
        SUB
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

    private fun resolvePsiClass(namePath: String, relativePath: String): com.intellij.psi.PsiClass? {
        val record = reader.findUniqueDeclaration(namePath, relativePath) ?: return null
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
            put("namePath", reader.computeNamePath(psiClass as PsiNamedElement) ?: (psiClass.name ?: qualifiedName))
            put("relativePath", relativePath)
            put("type", if (psiClass.isInterface) "interface" else "class")
            reader.buildTextRange(psiClass)?.let { put("textRange", it) }
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
}
