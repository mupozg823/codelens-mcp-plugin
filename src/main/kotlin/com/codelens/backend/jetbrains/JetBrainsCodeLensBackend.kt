package com.codelens.backend.jetbrains

import com.codelens.backend.CodeLensBackend
import com.codelens.model.FileEntry
import com.codelens.model.FileReadResult
import com.codelens.model.ModificationResult
import com.codelens.model.ReferenceInfo
import com.codelens.model.SearchResult
import com.codelens.model.SymbolInfo
import com.codelens.serena.SerenaCompatSymbols
import com.codelens.services.FileService
import com.codelens.services.ModificationService
import com.codelens.services.ReferenceService
import com.codelens.services.RenameScope
import com.codelens.services.SearchService
import com.codelens.services.SymbolService
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.application.ReadAction
import com.intellij.openapi.components.service
import com.intellij.openapi.project.Project
import com.intellij.psi.JavaPsiFacade
import com.intellij.psi.PsiClass
import com.intellij.psi.search.GlobalSearchScope
import com.intellij.psi.search.LocalSearchScope
import com.intellij.psi.search.searches.ClassInheritorsSearch
import com.intellij.refactoring.rename.RenameProcessor
import org.jetbrains.kotlin.asJava.classes.KtLightClass
import org.jetbrains.kotlin.psi.KtClass

class JetBrainsCodeLensBackend(private val project: Project) : CodeLensBackend {

    override val backendId: String = "jetbrains"
    override val languageBackendName: String = "JetBrains"
    private val serenaCompat by lazy { SerenaCompatSymbols(project) }

    override fun getSymbolsOverview(path: String, depth: Int): List<SymbolInfo> {
        return project.service<SymbolService>().getSymbolsOverview(path, depth)
    }

    override fun findSymbol(
        name: String,
        filePath: String?,
        includeBody: Boolean,
        exactMatch: Boolean
    ): List<SymbolInfo> {
        return project.service<SymbolService>().findSymbol(name, filePath, includeBody, exactMatch)
    }

    override fun findReferencingSymbols(
        symbolName: String,
        filePath: String?,
        maxResults: Int
    ): List<ReferenceInfo> {
        return project.service<ReferenceService>().findReferencingSymbols(symbolName, filePath, maxResults)
    }

    override fun getTypeHierarchy(
        fullyQualifiedName: String,
        hierarchyType: String,
        depth: Int
    ): Map<String, Any?> {
        return ReadAction.compute<Map<String, Any?>, Exception> {
            val psiClass = findPsiClass(fullyQualifiedName)
                ?: return@compute mapOf("error" to "Class not found: $fullyQualifiedName")

            val effectiveDepth = if (depth <= 0) Int.MAX_VALUE else depth
            val result = mutableMapOf<String, Any?>(
                "class_name" to psiClass.name,
                "fully_qualified_name" to psiClass.qualifiedName,
                "kind" to getClassKind(psiClass),
                "members" to getMembers(psiClass),
                "type_parameters" to getTypeParameters(psiClass)
            )
            if (hierarchyType == "super" || hierarchyType == "both") {
                result["supertypes"] = getSupertypesRecursive(psiClass, effectiveDepth)
            }
            if (hierarchyType == "sub" || hierarchyType == "both") {
                result["subtypes"] = getSubtypesRecursive(psiClass, effectiveDepth)
            }
            result
        }
    }

    override fun replaceSymbolBody(
        symbolName: String,
        filePath: String,
        newBody: String
    ): ModificationResult {
        if (isNamePathSelector(symbolName)) {
            return try {
                serenaCompat.replaceSymbolBody(symbolName.removePrefix("/"), filePath, newBody)
                ModificationResult(
                    success = true,
                    message = "Replaced body of '$symbolName' in $filePath",
                    filePath = filePath,
                    newContent = newBody
                )
            } catch (e: Exception) {
                ModificationResult(false, e.message ?: "replace_symbol_body failed")
            }
        }
        return project.service<ModificationService>().replaceSymbolBody(symbolName, filePath, newBody)
    }

    override fun insertAfterSymbol(
        symbolName: String,
        filePath: String,
        content: String
    ): ModificationResult {
        if (isNamePathSelector(symbolName)) {
            return try {
                serenaCompat.insertAfterSymbol(symbolName.removePrefix("/"), filePath, content)
                ModificationResult(
                    success = true,
                    message = "Inserted content after '$symbolName' in $filePath",
                    filePath = filePath
                )
            } catch (e: Exception) {
                ModificationResult(false, e.message ?: "insert_after_symbol failed")
            }
        }
        return project.service<ModificationService>().insertAfterSymbol(symbolName, filePath, content)
    }

    override fun insertBeforeSymbol(
        symbolName: String,
        filePath: String,
        content: String
    ): ModificationResult {
        if (isNamePathSelector(symbolName)) {
            return try {
                serenaCompat.insertBeforeSymbol(symbolName.removePrefix("/"), filePath, content)
                ModificationResult(
                    success = true,
                    message = "Inserted content before '$symbolName' in $filePath",
                    filePath = filePath
                )
            } catch (e: Exception) {
                ModificationResult(false, e.message ?: "insert_before_symbol failed")
            }
        }
        return project.service<ModificationService>().insertBeforeSymbol(symbolName, filePath, content)
    }

    override fun renameSymbol(
        symbolName: String,
        filePath: String,
        newName: String,
        scope: RenameScope
    ): ModificationResult {
        if (isNamePathSelector(symbolName)) {
            return try {
                val target = serenaCompat.resolveNamedElement(symbolName.removePrefix("/"), filePath)
                    ?: return ModificationResult(false, "Symbol '$symbolName' not found in $filePath")
                ApplicationManager.getApplication().invokeAndWait {
                    val searchScope = when (scope) {
                        RenameScope.FILE -> LocalSearchScope(target.containingFile)
                        RenameScope.PROJECT -> GlobalSearchScope.projectScope(project)
                    }
                    val processor = RenameProcessor(project, target, newName, searchScope, false, false)
                    processor.setPreviewUsages(false)
                    processor.run()
                }
                return ModificationResult(
                    success = true,
                    message = "Renamed '$symbolName' to '$newName'",
                    filePath = filePath
                )
            } catch (e: Exception) {
                return ModificationResult(false, e.message ?: "rename_symbol failed")
            }
        }
        return project.service<ModificationService>().renameSymbol(symbolName, filePath, newName, scope)
    }

    override fun readFile(path: String, startLine: Int?, endLine: Int?): FileReadResult {
        return project.service<FileService>().readFile(path, startLine, endLine)
    }

    override fun listDirectory(path: String, recursive: Boolean): List<FileEntry> {
        return project.service<FileService>().listDirectory(path, recursive)
    }

    override fun findFiles(pattern: String, baseDir: String?): List<String> {
        return project.service<FileService>().findFiles(pattern, baseDir)
    }

    override fun searchForPattern(
        pattern: String,
        fileGlob: String?,
        maxResults: Int,
        contextLines: Int
    ): List<SearchResult> {
        return project.service<SearchService>().searchForPattern(pattern, fileGlob, maxResults, contextLines)
    }

    private fun findPsiClass(fqn: String): PsiClass? {
        return JavaPsiFacade.getInstance(project).findClass(fqn, GlobalSearchScope.projectScope(project))
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

    private fun getSupertypesRecursive(psiClass: PsiClass, depth: Int): List<Map<String, Any?>> {
        if (depth <= 0) return emptyList()
        return try {
            psiClass.supers
                .filter { it.qualifiedName != "java.lang.Object" }
                .map { superClass ->
                    val entry = mutableMapOf<String, Any?>(
                        "name" to (superClass.name ?: ""),
                        "qualified_name" to (superClass.qualifiedName ?: ""),
                        "kind" to if (superClass.isInterface) "interface" else "class"
                    )
                    if (depth > 1) {
                        val children = getSupertypesRecursive(superClass, depth - 1)
                        if (children.isNotEmpty()) entry["supertypes"] = children
                    }
                    entry
                }
        } catch (_: Exception) {
            emptyList()
        }
    }

    private fun getSubtypesRecursive(psiClass: PsiClass, depth: Int): List<Map<String, Any?>> {
        if (depth <= 0) return emptyList()
        return try {
            ClassInheritorsSearch.search(psiClass, GlobalSearchScope.projectScope(project), true)
                .map { subClass ->
                    val entry = mutableMapOf<String, Any?>(
                        "name" to (subClass.name ?: ""),
                        "qualified_name" to (subClass.qualifiedName ?: "")
                    )
                    if (depth > 1) {
                        val children = getSubtypesRecursive(subClass, depth - 1)
                        if (children.isNotEmpty()) entry["subtypes"] = children
                    }
                    entry
                }
        } catch (_: Exception) {
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
        return psiClass.typeParameters.map { typeParam ->
            mapOf(
                "name" to (typeParam.name ?: ""),
                "bounds" to typeParam.superTypes.joinToString(", ") { it.presentableText }
            )
        }
    }

    private fun kotlinOriginOf(psiClass: PsiClass): KtClass? {
        return when (psiClass) {
            is KtLightClass -> psiClass.kotlinOrigin as? KtClass
            else -> psiClass.navigationElement as? KtClass
        }
    }

    private fun isNamePathSelector(selector: String): Boolean = selector.contains("/")
}
