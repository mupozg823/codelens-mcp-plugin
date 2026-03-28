package com.codelens.serena

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiNamedElement

internal class SerenaSymbolEditor(
    private val project: Project,
    private val reader: SerenaSymbolReader
) {

    fun replaceSymbolBody(namePath: String, relativePath: String, body: String) {
        val element = com.intellij.openapi.application.ReadAction.compute<PsiNamedElement, Exception> {
            reader.findUniqueDeclaration(namePath, relativePath)?.element
                ?: throw IllegalArgumentException("No symbol '$namePath' found in '$relativePath'")
        }
        ApplicationManager.getApplication().invokeAndWait {
            WriteCommandAction.runWriteCommandAction(project) {
                val document = PsiDocumentManager.getInstance(project).getDocument(element.containingFile)
                    ?: throw IllegalStateException("Cannot obtain document for '${element.containingFile.name}'")
                val range = element.textRange
                document.replaceString(range.startOffset, range.endOffset, body)
                PsiDocumentManager.getInstance(project).commitDocument(document)
            }
        }
    }

    fun insertAfterSymbol(namePath: String, relativePath: String, body: String) {
        val element = com.intellij.openapi.application.ReadAction.compute<PsiNamedElement, Exception> {
            reader.findUniqueDeclaration(namePath, relativePath)?.element
                ?: throw IllegalArgumentException("No symbol '$namePath' found in '$relativePath'")
        }
        ApplicationManager.getApplication().invokeAndWait {
            WriteCommandAction.runWriteCommandAction(project) {
                val document = PsiDocumentManager.getInstance(project).getDocument(element.containingFile)
                    ?: throw IllegalStateException("Cannot obtain document for '${element.containingFile.name}'")
                document.insertString(element.textRange.endOffset, "\n$body")
                PsiDocumentManager.getInstance(project).commitDocument(document)
            }
        }
    }

    fun insertBeforeSymbol(namePath: String, relativePath: String, body: String) {
        val element = com.intellij.openapi.application.ReadAction.compute<PsiNamedElement, Exception> {
            reader.findUniqueDeclaration(namePath, relativePath)?.element
                ?: throw IllegalArgumentException("No symbol '$namePath' found in '$relativePath'")
        }
        ApplicationManager.getApplication().invokeAndWait {
            WriteCommandAction.runWriteCommandAction(project) {
                val document = PsiDocumentManager.getInstance(project).getDocument(element.containingFile)
                    ?: throw IllegalStateException("Cannot obtain document for '${element.containingFile.name}'")
                document.insertString(element.textRange.startOffset, "$body\n")
                PsiDocumentManager.getInstance(project).commitDocument(document)
            }
        }
    }
}
