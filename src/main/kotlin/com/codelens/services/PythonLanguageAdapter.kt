package com.codelens.services

import com.codelens.model.SymbolInfo
import com.codelens.model.SymbolKind
import com.codelens.util.PsiUtils
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.PsiTreeUtil

/**
 * Python PSI adapter using reflection.
 * No compile-time dependency on PythonCore — resolves classes at runtime via Class.forName().
 * If PythonCore is not installed, [supports] returns false and GenericLanguageAdapter handles Python files.
 */
class PythonLanguageAdapter : LanguageAdapter {

    override val languageId: String = "Python"

    // Lazy-loaded PSI classes via reflection — Class.forName called once, cached thereafter
    private val pyFileClass: Class<*>? by lazy { tryLoad("com.jetbrains.python.psi.PyFile") }
    private val pyClassClass: Class<*>? by lazy { tryLoad("com.jetbrains.python.psi.PyClass") }
    private val pyFunctionClass: Class<*>? by lazy { tryLoad("com.jetbrains.python.psi.PyFunction") }
    private val pyTargetExprClass: Class<*>? by lazy { tryLoad("com.jetbrains.python.psi.PyTargetExpression") }
    private val pyAssignmentClass: Class<*>? by lazy { tryLoad("com.jetbrains.python.psi.PyAssignmentStatement") }
    private val pyStatementListClass: Class<*>? by lazy { tryLoad("com.jetbrains.python.psi.PyStatementList") }

    private val available: Boolean by lazy { pyFileClass != null }

    override fun supports(psiFile: PsiFile): Boolean {
        return available && pyFileClass!!.isInstance(psiFile)
    }

    override fun extractSymbols(psiFile: PsiFile, depth: Int): List<SymbolInfo> {
        if (!supports(psiFile)) return emptyList()
        val filePath = psiFile.virtualFile?.path ?: ""
        val symbols = mutableListOf<SymbolInfo>()

        for (child in psiFile.children) {
            extractElement(child, filePath, depth, 0)?.let { symbols.add(it) }
        }
        return symbols
    }

    private fun extractElement(
        element: PsiElement,
        filePath: String,
        maxDepth: Int,
        currentDepth: Int
    ): SymbolInfo? {
        return when {
            pyClassClass?.isInstance(element) == true -> extractClass(element, filePath, maxDepth, currentDepth)
            pyFunctionClass?.isInstance(element) == true -> extractFunction(element, filePath)
            pyAssignmentClass?.isInstance(element) == true -> extractAssignment(element, filePath)
            else -> null
        }
    }

    private fun extractClass(
        element: PsiElement,
        filePath: String,
        maxDepth: Int,
        currentDepth: Int
    ): SymbolInfo {
        val name = getNameOf(element) ?: "<anonymous>"
        val children = mutableListOf<SymbolInfo>()

        if (currentDepth < maxDepth) {
            // Find PyStatementList (class body) and iterate its children
            val body = PsiTreeUtil.getChildOfType(element, pyStatementListClass?.let {
                @Suppress("UNCHECKED_CAST")
                it as Class<PsiElement>
            } ?: PsiElement::class.java)

            if (body != null) {
                for (child in body.children) {
                    extractElement(child, filePath, maxDepth, currentDepth + 1)?.let {
                        children.add(it)
                    }
                }
            }
        }

        val superClasses = callStringListMethod(element, "getSuperClassExpressionList")
        val extendsStr = if (superClasses.isNotEmpty()) "(${superClasses.joinToString(", ")})" else ""

        return SymbolInfo(
            name = name,
            kind = SymbolKind.CLASS,
            filePath = filePath,
            line = PsiUtils.getLineNumber(element),
            column = PsiUtils.getColumnNumber(element),
            signature = "class $name$extendsStr",
            children = children,
            documentation = PsiUtils.extractDocumentation(element)
        )
    }

    private fun extractFunction(element: PsiElement, filePath: String): SymbolInfo {
        val name = getNameOf(element) ?: "<anonymous>"
        val params = callStringMethod(element, "getParameterList") ?: ""
        val returnType = callStringMethod(element, "getAnnotation")
        val async = callBooleanMethod(element, "isAsync")
        val asyncPrefix = if (async) "async " else ""
        val returnSuffix = if (returnType != null) " -> $returnType" else ""

        // Determine if method (inside class) or top-level function
        val kind = if (element.parent?.let { pyStatementListClass?.isInstance(it) } == true &&
            element.parent?.parent?.let { pyClassClass?.isInstance(it) } == true
        ) SymbolKind.METHOD else SymbolKind.FUNCTION

        return SymbolInfo(
            name = name,
            kind = kind,
            filePath = filePath,
            line = PsiUtils.getLineNumber(element),
            column = PsiUtils.getColumnNumber(element),
            signature = "${asyncPrefix}def $name($params)$returnSuffix",
            documentation = PsiUtils.extractDocumentation(element)
        )
    }

    private fun extractAssignment(element: PsiElement, filePath: String): SymbolInfo? {
        // PyAssignmentStatement → targets[0] is the variable name
        val targets = try {
            val method = element.javaClass.getMethod("getTargets")
            @Suppress("UNCHECKED_CAST")
            val result = method.invoke(element) as? Array<*>
            result?.firstOrNull() as? PsiElement
        } catch (e: Exception) {
            null
        } ?: return null

        val name = getNameOf(targets) ?: targets.text?.substringBefore(".")?.trim() ?: return null
        // Skip private/dunder assignments at module level unless they look like constants
        if (name.startsWith("_") && !name.all { it == '_' || it.isUpperCase() }) return null

        return SymbolInfo(
            name = name,
            kind = if (name == name.uppercase()) SymbolKind.CONSTANT else SymbolKind.PROPERTY,
            filePath = filePath,
            line = PsiUtils.getLineNumber(element),
            column = PsiUtils.getColumnNumber(element),
            signature = "$name = ...",
            documentation = PsiUtils.extractDocumentation(element)
        )
    }

    override fun classifyElement(element: PsiElement): SymbolKind? = when {
        pyClassClass?.isInstance(element) == true -> SymbolKind.CLASS
        pyFunctionClass?.isInstance(element) == true -> {
            if (element.parent?.let { pyStatementListClass?.isInstance(it) } == true &&
                element.parent?.parent?.let { pyClassClass?.isInstance(it) } == true
            ) SymbolKind.METHOD else SymbolKind.FUNCTION
        }
        pyTargetExprClass?.isInstance(element) == true -> SymbolKind.PROPERTY
        else -> null
    }

    override fun isDeclaration(element: PsiElement): Boolean = when {
        pyClassClass?.isInstance(element) == true -> true
        pyFunctionClass?.isInstance(element) == true -> true
        pyTargetExprClass?.isInstance(element) == true -> true
        else -> false
    }

    override fun getBodyText(element: PsiElement): String? {
        // Try to get statement list (body) of class/function
        val body = PsiTreeUtil.getChildOfType(element, pyStatementListClass?.let {
            @Suppress("UNCHECKED_CAST")
            it as Class<PsiElement>
        } ?: return element.text)
        return body?.text ?: element.text
    }

    override fun buildSignature(element: PsiElement): String = when {
        pyClassClass?.isInstance(element) == true -> {
            val name = getNameOf(element) ?: "?"
            "class $name"
        }
        pyFunctionClass?.isInstance(element) == true -> {
            val name = getNameOf(element) ?: "?"
            "def $name(...)"
        }
        else -> PsiUtils.buildSignature(element)
    }

    // --- Reflection helpers ---

    private fun tryLoad(className: String): Class<*>? = try {
        Class.forName(className)
    } catch (e: ClassNotFoundException) {
        null
    }

    private fun getNameOf(element: PsiElement): String? = try {
        element.javaClass.getMethod("getName").invoke(element) as? String
    } catch (e: Exception) {
        null
    }

    private fun callStringMethod(element: PsiElement, methodName: String): String? = try {
        val result = element.javaClass.getMethod(methodName).invoke(element)
        result?.toString()?.takeIf { it.isNotBlank() && it != "null" }
    } catch (e: Exception) {
        null
    }

    private fun callBooleanMethod(element: PsiElement, methodName: String): Boolean = try {
        element.javaClass.getMethod(methodName).invoke(element) as? Boolean ?: false
    } catch (e: Exception) {
        false
    }

    private fun callStringListMethod(element: PsiElement, methodName: String): List<String> = try {
        val result = element.javaClass.getMethod(methodName).invoke(element)
        when (result) {
            is Array<*> -> result.mapNotNull { (it as? PsiElement)?.text }
            is Collection<*> -> result.mapNotNull { (it as? PsiElement)?.text }
            else -> emptyList()
        }
    } catch (e: Exception) {
        emptyList()
    }
}
