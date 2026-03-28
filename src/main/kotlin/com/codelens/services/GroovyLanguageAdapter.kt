package com.codelens.services

import com.codelens.model.SymbolInfo
import com.codelens.model.SymbolKind
import com.codelens.util.PsiUtils
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import org.jetbrains.plugins.groovy.lang.psi.GroovyFile
import org.jetbrains.plugins.groovy.lang.psi.api.statements.GrField
import org.jetbrains.plugins.groovy.lang.psi.api.statements.GrVariable
import org.jetbrains.plugins.groovy.lang.psi.api.statements.typedef.GrTypeDefinition
import org.jetbrains.plugins.groovy.lang.psi.api.statements.typedef.members.GrMethod

/**
 * Groovy-specific PSI adapter.
 * Uses Groovy PSI classes (GrTypeDefinition, GrMethod, GrField) for precise symbol extraction.
 * Only loaded when the Groovy plugin is available (bundled in IntelliJ Ultimate).
 */
class GroovyLanguageAdapter : LanguageAdapter {

    override val languageId: String = "Groovy"

    override fun supports(psiFile: PsiFile): Boolean = psiFile is GroovyFile

    override fun extractSymbols(psiFile: PsiFile, depth: Int): List<SymbolInfo> {
        if (psiFile !is GroovyFile) return emptyList()

        val symbols = mutableListOf<SymbolInfo>()
        val filePath = psiFile.virtualFile?.path ?: ""

        for (typeDefinition in psiFile.typeDefinitions) {
            symbols.add(typeDefToSymbol(typeDefinition, filePath, depth, 0))
        }

        // Top-level methods (scripts)
        for (method in psiFile.methods) {
            symbols.add(
                SymbolInfo(
                    name = method.name,
                    kind = SymbolKind.FUNCTION,
                    filePath = filePath,
                    line = PsiUtils.getLineNumber(method),
                    column = PsiUtils.getColumnNumber(method),
                    signature = buildMethodSignature(method),
                    documentation = PsiUtils.extractDocumentation(method)
                )
            )
        }

        return symbols
    }

    private fun typeDefToSymbol(
        typeDef: GrTypeDefinition,
        filePath: String,
        maxDepth: Int,
        currentDepth: Int
    ): SymbolInfo {
        val children = mutableListOf<SymbolInfo>()

        if (currentDepth < maxDepth) {
            for (method in typeDef.codeMethods) {
                children.add(
                    SymbolInfo(
                        name = method.name,
                        kind = if (method.isConstructor) SymbolKind.CONSTRUCTOR else SymbolKind.METHOD,
                        filePath = filePath,
                        line = PsiUtils.getLineNumber(method),
                        column = PsiUtils.getColumnNumber(method),
                        signature = buildMethodSignature(method),
                        documentation = PsiUtils.extractDocumentation(method)
                    )
                )
            }

            for (field in typeDef.codeFields) {
                children.add(
                    SymbolInfo(
                        name = field.name,
                        kind = SymbolKind.FIELD,
                        filePath = filePath,
                        line = PsiUtils.getLineNumber(field),
                        column = PsiUtils.getColumnNumber(field),
                        signature = buildFieldSignature(field),
                        documentation = PsiUtils.extractDocumentation(field)
                    )
                )
            }

            for (innerClass in typeDef.codeInnerClasses) {
                if (innerClass is GrTypeDefinition) {
                    children.add(typeDefToSymbol(innerClass, filePath, maxDepth, currentDepth + 1))
                }
            }
        }

        val kind = when {
            typeDef.isInterface -> SymbolKind.INTERFACE
            typeDef.isEnum -> SymbolKind.ENUM
            typeDef.isAnnotationType -> SymbolKind.ANNOTATION
            typeDef.isTrait -> SymbolKind.INTERFACE
            else -> SymbolKind.CLASS
        }

        return SymbolInfo(
            name = typeDef.name ?: "<anonymous>",
            kind = kind,
            filePath = filePath,
            line = PsiUtils.getLineNumber(typeDef),
            column = PsiUtils.getColumnNumber(typeDef),
            signature = buildClassSignature(typeDef),
            children = children,
            documentation = PsiUtils.extractDocumentation(typeDef)
        )
    }

    override fun classifyElement(element: PsiElement): SymbolKind? = when (element) {
        is GrTypeDefinition -> when {
            element.isInterface -> SymbolKind.INTERFACE
            element.isEnum -> SymbolKind.ENUM
            element.isAnnotationType -> SymbolKind.ANNOTATION
            element.isTrait -> SymbolKind.INTERFACE
            else -> SymbolKind.CLASS
        }
        is GrMethod -> if (element.isConstructor) SymbolKind.CONSTRUCTOR else SymbolKind.METHOD
        is GrField -> SymbolKind.FIELD
        is GrVariable -> SymbolKind.VARIABLE
        else -> null
    }

    override fun isDeclaration(element: PsiElement): Boolean = when (element) {
        is GrTypeDefinition, is GrMethod, is GrField, is GrVariable -> true
        else -> false
    }

    override fun getBodyText(element: PsiElement): String? = when (element) {
        is GrMethod -> element.block?.text
        is GrTypeDefinition -> element.body?.text
        else -> element.text
    }

    override fun buildSignature(element: PsiElement): String = when (element) {
        is GrTypeDefinition -> buildClassSignature(element)
        is GrMethod -> buildMethodSignature(element)
        is GrField -> buildFieldSignature(element)
        else -> PsiUtils.buildSignature(element)
    }

    private fun buildClassSignature(typeDef: GrTypeDefinition): String {
        val keyword = when {
            typeDef.isInterface -> "interface"
            typeDef.isEnum -> "enum"
            typeDef.isTrait -> "trait"
            typeDef.isAnnotationType -> "@interface"
            else -> "class"
        }
        val extends = typeDef.extendsClause?.text?.let { " extends $it" } ?: ""
        val implements = typeDef.implementsClause?.text?.let { " implements $it" } ?: ""
        return "$keyword ${typeDef.name}$extends$implements"
    }

    private fun buildMethodSignature(method: GrMethod): String {
        val params = method.parameters.joinToString(", ") { param ->
            buildString {
                param.type.presentableText.let { append("$it ") }
                append(param.name ?: "?")
            }
        }
        val returnType = method.returnType?.presentableText?.let { "$it " } ?: "def "
        return "$returnType${method.name}($params)"
    }

    private fun buildFieldSignature(field: GrField): String {
        val type = field.typeGroovy?.presentableText ?: "def"
        return "$type ${field.name}"
    }
}
