package com.codelens.model

data class FileReadResult(
    val content: String,
    val totalLines: Int,
    val filePath: String
)

data class FileEntry(
    val name: String,
    val type: String, // "file" or "directory"
    val path: String,
    val size: Long? = null
)
