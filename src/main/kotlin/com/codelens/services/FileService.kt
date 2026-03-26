package com.codelens.services

import com.codelens.model.FileEntry
import com.codelens.model.FileReadResult
import com.intellij.openapi.components.Service

interface FileService {
    fun readFile(path: String, startLine: Int? = null, endLine: Int? = null): FileReadResult
    fun listDirectory(path: String, recursive: Boolean = false): List<FileEntry>
    fun findFiles(pattern: String, baseDir: String? = null): List<String>
}
