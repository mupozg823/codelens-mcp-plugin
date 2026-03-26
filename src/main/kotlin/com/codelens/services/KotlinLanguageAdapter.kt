package com.codelens.services

import com.codelens.model.SymbolInfo
import com.codelens.model.SymbolKind
import com.codelens.util.PsiUtils
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import org.jetbrains.kotlin.psi.*

/**
 * Kotlin-specific PSI adapter.
 * Uses Kotlin PSI classes (KtClass, KtFunction, KtProperty) for precise symbol extraction.
 */
class KotlinLanguageAdapter : LanguageAdapter {

    override val languageId: String = "kotlin"

    override fun supports(psiFile: PsiFile): Boolean {
        return psiFile is KtFile
    }

    override fun extractSymbols(psiFile: PsiFile, depth: Int): List<SymbolInfo> {
        if (psiFile !is KtFile) return emptyList()

        val symbols = mutableListOf<SymbolInfo>()
        val filePath = psiFile.virtualFile?.path ?: ""

        for (declaration in psiFile.declarations) {
            extractDeclaration(declaration, filePath, depth, 0)?.let { symbols.add(it) }
        }

        return symbols
    }

    private fun extractDeclaration(
        element: KtDeclaration,
        filePath: String,
        maxDepth: Int,
        currentDepth: Int
    ): SymbolInfo? {
        val name = element.name ?: return null

        val (kind, children) = when (element) {
            is KtClass -> {
                val kind = when {
                    element.isInterface() -> SymbolKind.INTERFACE
                    element.isEnum() -> SymbolKind.ENUM
                    element.isAnnotation() -> SymbolKind.ANNOTATION
                    else -> SymbolKind.CLASS
                }
                val children = if (currentDepth < maxDepth) {
                    extractClassMembers(element, filePath, maxDepth, currentDepth)
                } else emptyList()
                kind to children
            }
            is KtObjectDeclaration -> {
                val kind = if (element.isCompanion()) SymbolKind.COMPANION_OBJECT else SymbolKind.OBJECT
                val children = if (currentDepth < maxDepth) {
                    extractClassMembers(element, filePath, maxDepth, currentDepth)
                } else emptyList()
                kind to children
            }
            is KtNamedFunction -> SymbolKind.FUNCTION to emptyList()
            is KtProperty -> SymbolKind.PROPERTY to emptyList()
            is KtTypeAlias -> SymbolKind.TYPE_ALIAS to emptyList()
            else -> SymbolKind.UNKNOWN to emptyList()
        }

        return SymbolInfo(
            name = name,
            kind = kind,
            filePath = filePath,
            line = PsiUtils.getLineNumber(element),
            column = PsiUtils.getColumnNumber(element),
            signature = buildSignature(element),
            children = children,
            documentation = PsiUtils.extractDocumentation(element)
        )
    }

    private fun extractClassMembers(
        classLike: KtClassOrObject,
        filePath: String,
        maxDepth: Int,
        currentDepth: Int
    ): List<SymbolInfo> {
        val members = mutableListOf<SymbolInfo>()

        for (declaration in classLike.declarations) {
            extractDeclaration(declaration, filePath, maxDepth, currentDepth + 1)?.let {
                members.add(it)
            }
        }

        // Primary constructor parameters (val/var)
        if (classLike is KtClass) {
            classLike.primaryConstructorParameters
                .filter { it.hasValOrVar() }
                .forEach { param ->
                    members.add(
                        SymbolInfo(
                            name = param.name ?: return@forEach,
                            kind = SymbolKind.PROPERTY,
                            filePath = filePath,
                            line = PsiUtils.getLineNumber(param),
                            column = PsiUtils.getColumnNumber(param),
                            signature = buildParameterSignature(param)
                        )
                    )
                }
        }

        return members
    }

    override fun classifyElement(element: PsiElement): SymbolKind? = when (element) {
        is KtClass -> when {
            element.isInterface() -> SymbolKind.INTERFACE
            element.isEnum() -> SymbolKind.ENUM
            else -> SymbolKind.CLASS
        }
        is KtObjectDeclaration -> if (element.isCompanion()) SymbolKind.COMPANION_OBJECT else SymbolKind.OBJECT
        is KtNamedFunction -> SymbolKind.FUNCTION
        is KtProperty -> SymbolKind.PROPERTY
        is KtTypeAlias -> SymbolKind.TYPE_ALIAS
        else -> null
    }

    override fun isDeclaration(element: PsiElement): Boolean = element is KtDeclaration

    override fun getBodyText(element: PsiElement): String? = when (element) {
        is KtNamedFunction -> element.bodyExpression?.text ?: element.bodyBlockExpression?.text
        is KtClass -> element.body?.text
        is KtProperty -> element.initializer?.text ?: element.getter?.text
        else -> null
    }

    override fun buildSignature(element: PsiElement): String = when (element) {
        is KtClass -> buildClassSignature(element)
        is KtObjectDeclaration -> buildObjectSignature(element)
        is KtNamedFunction -> buildFunctionSignature(element)
        is KtProperty -> buildPropertySignature(element)
        is KtTypeAlias -> "typealias ${element.name} = ${element.getTypeReference()?.text ?: "?"}"
        else -> PsiUtils.buildSignature(element)
    }

    private fun buildClassSignature(ktClass: KtClass): String {
        val sb = StringBuilder()
        ktClass.modifierList?.let { mods ->
            val modText = mods.text.trim()
            if (modText.isNotEmpty()) sb.append("$modText ")
        }
        when {
            ktClass.isInterface() -> sb.append("interface ")
            ktClass.isEnum() -> sb.append("enum class ")
            ktClass.isAnnotation() -> sb.append("annotation class ")
            ktClass.isData() -> sb.append("data class ")
            ktClass.isSealed() -> sb.append("sealed class ")
            ktClass.isValue() -> sb.append("value class ")
            else -> sb.append("class ")
        }
        sb.append(ktClass.name)
        ktClass.typeParameterList?.let { typeParams ->
            sb.append(typeParams.text)
        }
        ktClass.primaryConstructor?.valueParameterList?.let { params ->
            sb.append(params.text.take(100))
            if (params.text.length > 100) sb.append("...")
        }
        val superTypes = ktClass.superTypeListEntries
        if (superTypes.isNotEmpty()) {
            sb.append(" : ")
            sb.append(superTypes.joinToString(", ") { it.text.take(50) })
        }
        return sb.toString().trim()
    }

    private fun buildObjectSignature(obj: KtObjectDeclaration): String {
        val prefix = if (obj.isCompanion()) "companion object" else "object"
        val name = obj.name?.let { " $it" } ?: ""
        val superTypes = obj.superTypeListEntries
        val extends = if (superTypes.isNotEmpty()) {
            " : " + superTypes.joinToString(", ") { it.text.take(50) }
        } else ""
        return "$prefix$name$extends"
    }

    private fun buildFunctionSignature(func: KtNamedFunction): String {
        val sb = StringBuilder()
        func.modifierList?.let { mods ->
            val keywords = mods.text.trim()
            if (keywords.isNotEmpty()) sb.append("$keywords ")
        }
        sb.append("fun ")
        func.typeParameterList?.let { sb.append("${it.text} ") }
        func.receiverTypeReference?.let { sb.append("${it.text}.") }
        sb.append(func.name ?: "<anonymous>")
        sb.append("(")
        sb.append(func.valueParameters.joinToString(", ") { param ->
            val paramName = param.name ?: "?"
            val paramType = param.typeReference?.text ?: "?"
            val default = if (param.hasDefaultValue()) " = ..." else ""
            "$paramName: $paramType$default"
        })
        sb.append(")")
        func.typeReference?.let { sb.append(": ${it.text}") }
        return sb.toString().trim()
    }

    private fun buildPropertySignature(prop: KtProperty): String {
        val sb = StringBuilder()
        prop.modifierList?.let { mods ->
            val keywords = mods.text.trim()
            if (keywords.isNotEmpty()) sb.append("$keywords ")
        }
        sb.append(if (prop.isVar) "var " else "val ")
        prop.receiverTypeReference?.let { sb.append("${it.text}.") }
        sb.append(prop.name ?: "<anonymous>")
        prop.typeReference?.let { sb.append(": ${it.text}") }
        return sb.toString().trim()
    }

    private fun buildParameterSignature(param: KtParameter): String {
        val keyword = if (param.isMutable) "var" else "val"
        val type = param.typeReference?.text ?: "?"
        return "$keyword ${param.name}: $type"
    }
}
