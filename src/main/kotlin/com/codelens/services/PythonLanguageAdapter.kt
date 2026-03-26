package com.codelens.services

import com.codelens.model.SymbolInfo
import com.codelens.model.SymbolKind
import com.codelens.util.PsiUtils
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.jetbrains.python.psi.*

/**
 * Language adapter for Python files.
 * Only loaded when the Python plugin (PythonCore or Pythonid) is installed.
 */
class PythonLanguageAdapter : LanguageAdapter {

    override val languageId: String = "Python"

    override fun supports(psiFile: PsiFile): Boolean = psiFile is PyFile

    override fun extractSymbols(psiFile: PsiFile, depth: Int): List<SymbolInfo> {
        if (psiFile !is PyFile) return emptyList()
        return extractFromStatements(psiFile.statements.toList(), psiFile.virtualFile?.path ?: "", depth, 0)
    }

    private fun extractFromStatements(
        statements: List<PyStatement>,
        filePath: String,
        maxDepth: Int,
        currentDepth: Int
    ): List<SymbolInfo> {
        val result = mutableListOf<SymbolInfo>()
        for (stmt in statements) {
            when (stmt) {
                is PyClass -> {
                    val children = if (currentDepth < maxDepth) {
                        extractFromStatements(
                            stmt.statementList.statements.toList(),
                            filePath, maxDepth, currentDepth + 1
                        )
                    } else emptyList()

                    result.add(
                        SymbolInfo(
                            name = stmt.name ?: continue,
                            kind = SymbolKind.CLASS,
                            filePath = filePath,
                            line = PsiUtils.getLineNumber(stmt),
                            column = PsiUtils.getColumnNumber(stmt),
                            signature = buildClassSignature(stmt),
                            children = children
                        )
                    )
                }
                is PyFunction -> {
                    val kind = if (stmt.containingClass != null) SymbolKind.METHOD else SymbolKind.FUNCTION
                    result.add(
                        SymbolInfo(
                            name = stmt.name ?: continue,
                            kind = kind,
                            filePath = filePath,
                            line = PsiUtils.getLineNumber(stmt),
                            column = PsiUtils.getColumnNumber(stmt),
                            signature = buildFunctionSignature(stmt)
                        )
                    )
                }
                is PyAssignmentStatement -> {
                    // Module-level or class-level assignments
                    for (target in stmt.targets) {
                        if (target is PyTargetExpression) {
                            result.add(
                                SymbolInfo(
                                    name = target.name ?: continue,
                                    kind = SymbolKind.VARIABLE,
                                    filePath = filePath,
                                    line = PsiUtils.getLineNumber(stmt),
                                    column = PsiUtils.getColumnNumber(stmt),
                                    signature = "${target.name} = ..."
                                )
                            )
                        }
                    }
                }
            }
        }
        return result
    }

    override fun classifyElement(element: PsiElement): SymbolKind? = when (element) {
        is PyClass -> SymbolKind.CLASS
        is PyFunction -> if (element.containingClass != null) SymbolKind.METHOD else SymbolKind.FUNCTION
        is PyTargetExpression -> SymbolKind.VARIABLE
        is PyNamedParameter -> SymbolKind.VARIABLE
        else -> null
    }

    override fun isDeclaration(element: PsiElement): Boolean = when (element) {
        is PyClass, is PyFunction, is PyTargetExpression -> true
        else -> false
    }

    override fun getBodyText(element: PsiElement): String? = when (element) {
        is PyFunction -> element.statementList?.text
        is PyClass -> element.statementList?.text
        else -> element.text
    }

    override fun buildSignature(element: PsiElement): String = when (element) {
        is PyClass -> buildClassSignature(element)
        is PyFunction -> buildFunctionSignature(element)
        else -> PsiUtils.buildSignature(element)
    }

    private fun buildClassSignature(cls: PyClass): String {
        val superClasses = cls.superClassExpressionList?.text?.let { "($it)" } ?: ""
        val decorators = cls.decoratorList?.decorators?.joinToString(" ") { "@${it.name ?: ""}" } ?: ""
        return buildString {
            if (decorators.isNotEmpty()) append("$decorators ")
            append("class ${cls.name}$superClasses")
        }
    }

    private fun buildFunctionSignature(func: PyFunction): String {
        val params = func.parameterList.parameters.joinToString(", ") { param ->
            buildString {
                append(param.name ?: "?")
                (param as? PyNamedParameter)?.annotation?.text?.let { append(": $it") }
                param.defaultValueText?.let { append(" = $it") }
            }
        }
        val returnType = func.annotation?.text?.let { " -> $it" } ?: ""
        val decorators = func.decoratorList?.decorators?.joinToString(" ") { "@${it.name ?: ""}" } ?: ""
        val async = if (func.isAsync) "async " else ""
        return buildString {
            if (decorators.isNotEmpty()) append("$decorators ")
            append("${async}def ${func.name}($params)$returnType")
        }
    }
}
