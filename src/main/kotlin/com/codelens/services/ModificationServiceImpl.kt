package com.codelens.services

import com.codelens.model.ModificationResult
import com.codelens.util.PsiUtils
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.application.ReadAction
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.project.DumbService
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.PsiFileFactory
import com.intellij.psi.PsiNamedElement
import com.intellij.psi.PsiWhiteSpace
import com.intellij.psi.search.GlobalSearchScope
import com.intellij.psi.search.LocalSearchScope
import com.intellij.refactoring.rename.RenameProcessor
import java.util.concurrent.CompletableFuture
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference

class ModificationServiceImpl(private val project: Project) : ModificationService {

    /**
     * Run a block on EDT using invokeAndWait (works in both real IDE and tests).
     * If already on EDT, runs immediately.
     */
    private fun <T> runOnEdt(block: () -> T): T {
        val app = ApplicationManager.getApplication()
        if (app.isDispatchThread) {
            return block()
        }
        val result = AtomicReference<T>()
        val error = AtomicReference<Exception>()
        app.invokeAndWait {
            try {
                result.set(block())
            } catch (e: Exception) {
                error.set(e)
            }
        }
        error.get()?.let { throw it }
        return result.get()
    }

    override fun replaceSymbolBody(
        symbolName: String,
        filePath: String,
        newBody: String
    ): ModificationResult {
        return executeModification("replace_symbol_body") {
            val resolvedPath = resolvePath(filePath)

            val lookupResult = ReadAction.compute<Triple<PsiFile, PsiNamedElement, PsiElement>?, Throwable> {
                val psiFile = PsiUtils.findPsiFile(project, resolvedPath) ?: return@compute null
                val target = PsiUtils.findElementByName(psiFile, symbolName, exactMatch = true, declarationsOnly = true)
                    .firstOrNull() ?: return@compute null
                val factory = PsiFileFactory.getInstance(project)
                val dummyFile = factory.createFileFromText(
                    "dummy.${psiFile.fileType.defaultExtension}",
                    psiFile.language,
                    newBody
                )
                val newElement = dummyFile.children.firstOrNull { it !is PsiWhiteSpace && it.text.isNotBlank() }
                    ?: return@compute null
                Triple(psiFile, target, newElement)
            } ?: return@executeModification ModificationResult(
                false, "Symbol '$symbolName' not found in $filePath"
            )

            val (psiFile, target, newElement) = lookupResult

            runOnEdt {
                WriteCommandAction.runWriteCommandAction(project, "CodeLens: Replace Symbol Body", null, {
                    target.replace(newElement)
                }, psiFile)
                PsiDocumentManager.getInstance(project).commitAllDocuments()
            }

            ModificationResult(
                success = true,
                message = "Replaced body of '$symbolName' in $filePath",
                filePath = resolvedPath,
                newContent = newBody
            )
        }
    }

    override fun insertAfterSymbol(
        symbolName: String,
        filePath: String,
        content: String
    ): ModificationResult {
        return executeModification("insert_after_symbol") {
            val resolvedPath = resolvePath(filePath)

            val lookupResult = ReadAction.compute<Pair<PsiFile, PsiNamedElement>?, Throwable> {
                val psiFile = PsiUtils.findPsiFile(project, resolvedPath) ?: return@compute null
                val target = PsiUtils.findElementByName(psiFile, symbolName, exactMatch = true, declarationsOnly = true)
                    .firstOrNull() ?: return@compute null
                Pair(psiFile, target)
            } ?: return@executeModification ModificationResult(
                false, "Symbol '$symbolName' not found in $filePath"
            )

            val (psiFile, target) = lookupResult

            runOnEdt {
                WriteCommandAction.runWriteCommandAction(project, "CodeLens: Insert After Symbol", null, {
                    val factory = PsiFileFactory.getInstance(project)
                    val dummyFile = factory.createFileFromText(
                        "dummy.${psiFile.fileType.defaultExtension}",
                        psiFile.language,
                        "\n$content"
                    )
                    val parent = target.parent
                    var anchor: PsiElement = target
                    for (child in dummyFile.children) {
                        anchor = parent.addAfter(child, anchor)
                    }
                }, psiFile)
                PsiDocumentManager.getInstance(project).commitAllDocuments()
            }

            ModificationResult(
                success = true,
                message = "Inserted content after '$symbolName' in $filePath",
                filePath = resolvedPath
            )
        }
    }

    override fun insertBeforeSymbol(
        symbolName: String,
        filePath: String,
        content: String
    ): ModificationResult {
        return executeModification("insert_before_symbol") {
            val resolvedPath = resolvePath(filePath)

            val lookupResult = ReadAction.compute<Pair<PsiFile, PsiNamedElement>?, Throwable> {
                val psiFile = PsiUtils.findPsiFile(project, resolvedPath) ?: return@compute null
                val target = PsiUtils.findElementByName(psiFile, symbolName, exactMatch = true, declarationsOnly = true)
                    .firstOrNull() ?: return@compute null
                Pair(psiFile, target)
            } ?: return@executeModification ModificationResult(
                false, "Symbol '$symbolName' not found in $filePath"
            )

            val (psiFile, target) = lookupResult

            runOnEdt {
                WriteCommandAction.runWriteCommandAction(project, "CodeLens: Insert Before Symbol", null, {
                    val factory = PsiFileFactory.getInstance(project)
                    val dummyFile = factory.createFileFromText(
                        "dummy.${psiFile.fileType.defaultExtension}",
                        psiFile.language,
                        content
                    )
                    val parent = target.parent
                    for (child in dummyFile.children.reversed()) {
                        parent.addBefore(child, target)
                    }
                    val nlFile = factory.createFileFromText("nl.txt", psiFile.language, "\n")
                    nlFile.firstChild?.let { parent.addBefore(it, target) }
                }, psiFile)
                PsiDocumentManager.getInstance(project).commitAllDocuments()
            }

            ModificationResult(
                success = true,
                message = "Inserted content before '$symbolName' in $filePath",
                filePath = resolvedPath
            )
        }
    }

    override fun renameSymbol(
        symbolName: String,
        filePath: String,
        newName: String,
        scope: RenameScope
    ): ModificationResult {
        return executeModification("rename_symbol") {
            val resolvedPath = resolvePath(filePath)

            val lookupResult = ReadAction.compute<Pair<PsiFile, PsiNamedElement>?, Throwable> {
                val psiFile = PsiUtils.findPsiFile(project, resolvedPath) ?: return@compute null
                val target = PsiUtils.findElementByName(psiFile, symbolName, exactMatch = true, declarationsOnly = true)
                    .firstOrNull() ?: return@compute null
                Pair(psiFile, target)
            } ?: return@executeModification ModificationResult(
                false, "Symbol '$symbolName' not found in $filePath"
            )

            val (psiFile, target) = lookupResult

            // RenameProcessor.run() needs EDT and manages its own write actions
            runOnEdt {
                val searchScope = when (scope) {
                    RenameScope.FILE -> LocalSearchScope(psiFile)
                    RenameScope.PROJECT -> GlobalSearchScope.projectScope(project)
                }
                val processor = RenameProcessor(
                    project, target, newName, searchScope,
                    false, false
                )
                processor.setPreviewUsages(false)
                processor.run()
                PsiDocumentManager.getInstance(project).commitAllDocuments()
            }

            ModificationResult(
                success = true,
                message = "Renamed '$symbolName' to '$newName'",
                filePath = resolvedPath
            )
        }
    }

    private fun executeModification(
        operationName: String,
        block: () -> ModificationResult
    ): ModificationResult {
        if (DumbService.getInstance(project).isDumb) {
            return ModificationResult(
                false,
                "IDE is currently indexing. Please wait and retry '$operationName'."
            )
        }
        return try {
            block()
        } catch (e: Exception) {
            ModificationResult(false, "$operationName failed: ${e.message}")
        }
    }

    private fun resolvePath(path: String): String {
        if (path.startsWith("/")) return path
        val basePath = project.basePath ?: return path
        return "$basePath/$path"
    }
}
