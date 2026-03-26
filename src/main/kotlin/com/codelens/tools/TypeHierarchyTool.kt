package com.codelens.tools

import com.intellij.openapi.application.ReadAction
import com.intellij.openapi.project.Project
import com.intellij.psi.JavaPsiFacade
import com.intellij.psi.PsiClass
import com.intellij.psi.search.GlobalSearchScope
import com.intellij.psi.search.searches.ClassInheritorsSearch
import org.jetbrains.kotlin.asJava.classes.KtLightClass
import org.jetbrains.kotlin.psi.KtClass

class TypeHierarchyTool : BaseMcpTool() {
    override val toolName = "get_type_hierarchy"
    override val description = "Get the type hierarchy (supertypes and subtypes) for a class"
    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "fully_qualified_name" to mapOf(
                "type" to "string",
                "description" to "Fully qualified class name (e.g., com.example.MyClass)"
            )
        ),
        "required" to listOf("fully_qualified_name")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        return try {
            val fqn = requireString(args, "fully_qualified_name")
            val result = ReadAction.compute<Map<String, Any?>, Exception> {
                getTypeHierarchy(project, fqn)
            }
            successResponse(result)
        } catch (e: Exception) {
            errorResponse("Failed to get type hierarchy: ${e.message}")
        }
    }

    private fun getTypeHierarchy(project: Project, fqn: String): Map<String, Any?> {
        val psiClass = findPsiClass(project, fqn)
            ?: return mapOf("error" to "Class not found: $fqn")

        return mapOf(
            "class_name" to psiClass.name,
            "fully_qualified_name" to psiClass.qualifiedName,
            "kind" to getClassKind(psiClass),
            "supertypes" to getSupertypes(psiClass),
            "subtypes" to getSubtypes(project, psiClass),
            "members" to getMembers(psiClass),
            "type_parameters" to getTypeParameters(psiClass)
        )
    }

    private fun findPsiClass(project: Project, fqn: String): PsiClass? {
        val scope = GlobalSearchScope.projectScope(project)
        val psiFacade = JavaPsiFacade.getInstance(project)
        return psiFacade.findClass(fqn, scope)
    }

    private fun getClassKind(psiClass: PsiClass): String {
        val kotlinOrigin = kotlinOriginOf(psiClass)
        return when {
            psiClass.isInterface -> "interface"
            psiClass.isEnum -> "enum"
            psiClass.isAnnotationType -> "annotation"
            kotlinOrigin?.isData() == true -> "data_class"
            else -> "class"
        }
    }

    private fun getSupertypes(psiClass: PsiClass): List<Map<String, String>> {
        return try {
            psiClass.supers.map { superClass ->
                mapOf(
                    "name" to (superClass.name ?: ""),
                    "qualified_name" to (superClass.qualifiedName ?: ""),
                    "kind" to if (superClass.isInterface) "interface" else "class"
                )
            }
        } catch (e: Exception) {
            emptyList()
        }
    }

    private fun getSubtypes(project: Project, psiClass: PsiClass): List<Map<String, String>> {
        return try {
            ClassInheritorsSearch.search(psiClass, GlobalSearchScope.projectScope(project), true)
                .map { subClass ->
                    mapOf(
                    "name" to (subClass.name ?: ""),
                    "qualified_name" to (subClass.qualifiedName ?: "")
                    )
                }
        } catch (e: Exception) {
            emptyList()
        }
    }

    private fun getMembers(psiClass: PsiClass): Map<String, List<String>> {
        val methods = psiClass.methods.map { method ->
            "${method.name}(${method.parameterList.parameters.joinToString(", ") { it.type.presentableText }})"
        }
        val fields = psiClass.fields.map { field ->
            "${field.name}: ${field.type.presentableText}"
        }
        val properties = kotlinOriginOf(psiClass)?.let { kotlinClass ->
            (
                kotlinClass.getProperties().mapNotNull { it.name } +
                    kotlinClass.primaryConstructorParameters
                        .filter { it.hasValOrVar() }
                        .mapNotNull { it.name }
                ).distinct()
        }.orEmpty()

        return mapOf(
            "methods" to methods,
            "fields" to fields,
            "properties" to properties
        )
    }

    private fun getTypeParameters(psiClass: PsiClass): List<Map<String, String>> {
        val typeParams = mutableListOf<Map<String, String>>()
        for (typeParam in psiClass.typeParameters) {
            typeParams.add(mapOf(
                "name" to (typeParam.name ?: ""),
                "bounds" to typeParam.superTypes.joinToString(", ") { it.presentableText }
            ))
        }
        return typeParams
    }

    private fun kotlinOriginOf(psiClass: PsiClass): KtClass? {
        return when (psiClass) {
            is KtLightClass -> psiClass.kotlinOrigin as? KtClass
            else -> psiClass.navigationElement as? KtClass
        }
    }
}
