package com.codelens.services

import com.codelens.model.SymbolInfo
import com.codelens.model.SymbolKind
import com.codelens.util.PsiUtils
import com.intellij.psi.*

/**
 * Java-specific PSI adapter.
 * Uses Java PSI classes (PsiClass, PsiMethod, PsiField) for precise symbol extraction.
 */
class JavaLanguageAdapter : LanguageAdapter {

    override val languageId: String = "JAVA"

    override fun supports(psiFile: PsiFile): Boolean {
        return psiFile is PsiJavaFile
    }

    override fun extractSymbols(psiFile: PsiFile, depth: Int): List<SymbolInfo> {
        if (psiFile !is PsiJavaFile) return emptyList()

        val symbols = mutableListOf<SymbolInfo>()
        val filePath = psiFile.virtualFile?.path ?: ""

        for (psiClass in psiFile.classes) {
            symbols.add(classToSymbol(psiClass, filePath, depth, 0))
        }

        return symbols
    }

    private fun classToSymbol(
        psiClass: PsiClass,
        filePath: String,
        maxDepth: Int,
        currentDepth: Int
    ): SymbolInfo {
        val children = mutableListOf<SymbolInfo>()

        if (currentDepth < maxDepth) {
            // Methods
            for (method in psiClass.methods) {
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

            // Fields
            for (field in psiClass.fields) {
                children.add(
                    SymbolInfo(
                        name = field.name,
                        kind = if (field.hasModifierProperty(PsiModifier.FINAL) &&
                            field.hasModifierProperty(PsiModifier.STATIC))
                            SymbolKind.CONSTANT else SymbolKind.FIELD,
                        filePath = filePath,
                        line = PsiUtils.getLineNumber(field),
                        column = PsiUtils.getColumnNumber(field),
                        signature = buildFieldSignature(field),
                        documentation = PsiUtils.extractDocumentation(field)
                    )
                )
            }

            // Inner classes
            for (innerClass in psiClass.innerClasses) {
                children.add(classToSymbol(innerClass, filePath, maxDepth, currentDepth + 1))
            }
        }

        val kind = when {
            psiClass.isInterface -> SymbolKind.INTERFACE
            psiClass.isEnum -> SymbolKind.ENUM
            psiClass.isAnnotationType -> SymbolKind.ANNOTATION
            else -> SymbolKind.CLASS
        }

        return SymbolInfo(
            name = psiClass.name ?: "<anonymous>",
            kind = kind,
            filePath = filePath,
            line = PsiUtils.getLineNumber(psiClass),
            column = PsiUtils.getColumnNumber(psiClass),
            signature = buildClassSignature(psiClass),
            children = children,
            documentation = PsiUtils.extractDocumentation(psiClass)
        )
    }

    private fun buildClassSignature(psiClass: PsiClass): String {
        val sb = StringBuilder()
        psiClass.modifierList?.let { mods ->
            if (mods.hasModifierProperty(PsiModifier.PUBLIC)) sb.append("public ")
            if (mods.hasModifierProperty(PsiModifier.ABSTRACT)) sb.append("abstract ")
            if (mods.hasModifierProperty(PsiModifier.FINAL)) sb.append("final ")
        }
        when {
            psiClass.isInterface -> sb.append("interface ")
            psiClass.isEnum -> sb.append("enum ")
            psiClass.isAnnotationType -> sb.append("@interface ")
            else -> sb.append("class ")
        }
        sb.append(psiClass.name)
        psiClass.typeParameterList?.let { typeParams ->
            if (typeParams.typeParameters.isNotEmpty()) {
                sb.append("<")
                sb.append(typeParams.typeParameters.joinToString(", ") { it.name ?: "?" })
                sb.append(">")
            }
        }
        psiClass.extendsList?.referenceElements?.firstOrNull()?.let {
            sb.append(" extends ${it.referenceName}")
        }
        val interfaces = psiClass.implementsList?.referenceElements ?: emptyArray()
        if (interfaces.isNotEmpty()) {
            val keyword = if (psiClass.isInterface) " extends " else " implements "
            sb.append(keyword)
            sb.append(interfaces.joinToString(", ") { it.referenceName ?: "?" })
        }
        return sb.toString().trim()
    }

    private fun buildMethodSignature(method: PsiMethod): String {
        val sb = StringBuilder()
        method.modifierList.let { mods ->
            if (mods.hasModifierProperty(PsiModifier.PUBLIC)) sb.append("public ")
            if (mods.hasModifierProperty(PsiModifier.PRIVATE)) sb.append("private ")
            if (mods.hasModifierProperty(PsiModifier.PROTECTED)) sb.append("protected ")
            if (mods.hasModifierProperty(PsiModifier.STATIC)) sb.append("static ")
            if (mods.hasModifierProperty(PsiModifier.ABSTRACT)) sb.append("abstract ")
        }
        if (!method.isConstructor) {
            method.returnType?.let { sb.append("${it.presentableText} ") }
        }
        sb.append(method.name)
        sb.append("(")
        sb.append(method.parameterList.parameters.joinToString(", ") { param ->
            "${param.type.presentableText} ${param.name}"
        })
        sb.append(")")
        val throwsList = method.throwsList.referenceElements
        if (throwsList.isNotEmpty()) {
            sb.append(" throws ")
            sb.append(throwsList.joinToString(", ") { it.referenceName ?: "?" })
        }
        return sb.toString().trim()
    }

    private fun buildFieldSignature(field: PsiField): String {
        val sb = StringBuilder()
        field.modifierList?.let { mods ->
            if (mods.hasModifierProperty(PsiModifier.PUBLIC)) sb.append("public ")
            if (mods.hasModifierProperty(PsiModifier.PRIVATE)) sb.append("private ")
            if (mods.hasModifierProperty(PsiModifier.STATIC)) sb.append("static ")
            if (mods.hasModifierProperty(PsiModifier.FINAL)) sb.append("final ")
        }
        sb.append("${field.type.presentableText} ${field.name}")
        return sb.toString().trim()
    }

    override fun classifyElement(element: PsiElement): SymbolKind? = when (element) {
        is PsiClass -> when {
            element.isInterface -> SymbolKind.INTERFACE
            element.isEnum -> SymbolKind.ENUM
            else -> SymbolKind.CLASS
        }
        is PsiMethod -> if (element.isConstructor) SymbolKind.CONSTRUCTOR else SymbolKind.METHOD
        is PsiField -> SymbolKind.FIELD
        is PsiLocalVariable -> SymbolKind.VARIABLE
        else -> null
    }

    override fun isDeclaration(element: PsiElement): Boolean = when (element) {
        is PsiClass, is PsiMethod, is PsiField, is PsiLocalVariable -> true
        else -> false
    }

    override fun getBodyText(element: PsiElement): String? {
        return when (element) {
            is PsiMethod -> element.body?.text
            is PsiClass -> element.text
            is PsiField -> element.text
            else -> null
        }
    }

    override fun buildSignature(element: PsiElement): String = when (element) {
        is PsiClass -> buildClassSignature(element)
        is PsiMethod -> buildMethodSignature(element)
        is PsiField -> buildFieldSignature(element)
        else -> PsiUtils.buildSignature(element)
    }
}
