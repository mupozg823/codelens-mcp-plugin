package com.codelens.services

import com.codelens.model.SymbolInfo
import com.codelens.model.SymbolKind
import com.codelens.util.PsiUtils
import com.intellij.lang.javascript.psi.*
import com.intellij.lang.javascript.psi.ecmal4.JSClass
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile

/**
 * Language adapter for JavaScript and TypeScript files.
 * Only loaded when the JavaScript plugin is installed (IntelliJ Ultimate / WebStorm).
 */
class JavaScriptLanguageAdapter : LanguageAdapter {

    override val languageId: String = "JavaScript"

    override fun supports(psiFile: PsiFile): Boolean = psiFile is JSFile

    override fun extractSymbols(psiFile: PsiFile, depth: Int): List<SymbolInfo> {
        if (psiFile !is JSFile) return emptyList()
        return extractFromElements(psiFile.children.toList(), psiFile.virtualFile?.path ?: "", depth, 0)
    }

    private fun extractFromElements(
        elements: List<PsiElement>,
        filePath: String,
        maxDepth: Int,
        currentDepth: Int
    ): List<SymbolInfo> {
        val result = mutableListOf<SymbolInfo>()
        for (element in elements) {
            when (element) {
                is JSClass -> {
                    val children = if (currentDepth < maxDepth) {
                        val members = element.members.toList()
                        extractFromElements(members, filePath, maxDepth, currentDepth + 1)
                    } else emptyList()

                    result.add(
                        SymbolInfo(
                            name = element.name ?: continue,
                            kind = classifyJSClass(element),
                            filePath = filePath,
                            line = PsiUtils.getLineNumber(element),
                            column = PsiUtils.getColumnNumber(element),
                            signature = buildClassSignature(element),
                            children = children
                        )
                    )
                }
                is JSFunction -> {
                    result.add(
                        SymbolInfo(
                            name = element.name ?: continue,
                            kind = SymbolKind.FUNCTION,
                            filePath = filePath,
                            line = PsiUtils.getLineNumber(element),
                            column = PsiUtils.getColumnNumber(element),
                            signature = buildFunctionSignature(element)
                        )
                    )
                }
                is JSVarStatement -> {
                    for (variable in element.variables) {
                        val kind = when {
                            element.text.startsWith("const") -> SymbolKind.CONSTANT
                            else -> SymbolKind.VARIABLE
                        }
                        result.add(
                            SymbolInfo(
                                name = variable.name ?: continue,
                                kind = kind,
                                filePath = filePath,
                                line = PsiUtils.getLineNumber(variable),
                                column = PsiUtils.getColumnNumber(variable),
                                signature = buildVariableSignature(variable, element)
                            )
                        )
                    }
                }
                is JSExpressionStatement -> {
                    // Handle module.exports = ... or exports.xxx = ...
                    // Skip for now — not a declaration
                }
                else -> {
                    // Recurse into container elements
                    if (element.children.isNotEmpty() && currentDepth < maxDepth) {
                        result.addAll(extractFromElements(element.children.toList(), filePath, maxDepth, currentDepth))
                    }
                }
            }
        }
        return result
    }

    private fun classifyJSClass(cls: JSClass): SymbolKind {
        val text = cls.text
        return when {
            text.contains("interface ") -> SymbolKind.INTERFACE
            text.contains("enum ") -> SymbolKind.ENUM
            else -> SymbolKind.CLASS
        }
    }

    override fun classifyElement(element: PsiElement): SymbolKind? = when (element) {
        is JSClass -> classifyJSClass(element)
        is JSFunction -> SymbolKind.FUNCTION
        is JSVariable -> if (element.parent?.text?.startsWith("const") == true) SymbolKind.CONSTANT else SymbolKind.VARIABLE
        is JSField -> SymbolKind.FIELD
        is JSProperty -> SymbolKind.PROPERTY
        else -> null
    }

    override fun isDeclaration(element: PsiElement): Boolean = when (element) {
        is JSClass, is JSFunction, is JSVariable, is JSField -> true
        else -> false
    }

    override fun getBodyText(element: PsiElement): String? = when (element) {
        is JSFunction -> element.block?.text
        is JSClass -> element.lastChild?.text
        else -> element.text
    }

    override fun buildSignature(element: PsiElement): String = when (element) {
        is JSClass -> buildClassSignature(element)
        is JSFunction -> buildFunctionSignature(element)
        is JSVariable -> buildVariableSignature(element, element.parent)
        else -> PsiUtils.buildSignature(element)
    }

    private fun buildClassSignature(cls: JSClass): String {
        val extends = cls.extendsList?.let { " extends ${it.text}" } ?: ""
        val implements = cls.implementsList?.let { " implements ${it.text}" } ?: ""
        return "class ${cls.name}$extends$implements"
    }

    private fun buildFunctionSignature(func: JSFunction): String {
        val params = func.parameters.joinToString(", ") { param ->
            buildString {
                append(param.name ?: "?")
                param.typeElement?.text?.let { append(": $it") }
            }
        }
        val returnType = func.returnTypeElement?.text?.let { ": $it" } ?: ""
        val async = if (func.isAsync) "async " else ""
        return "${async}function ${func.name ?: "anonymous"}($params)$returnType"
    }

    private fun buildVariableSignature(variable: JSVariable, parent: PsiElement?): String {
        val keyword = when {
            parent?.text?.startsWith("const") == true -> "const"
            parent?.text?.startsWith("let") == true -> "let"
            else -> "var"
        }
        val typeAnnotation = variable.typeElement?.text?.let { ": $it" } ?: ""
        return "$keyword ${variable.name}$typeAnnotation"
    }
}
