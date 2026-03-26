package com.codelens.tools.adapters

import com.codelens.tools.FindReferencingCodeSnippetsTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

/**
 * MCP Tool adapter for find_referencing_code_snippets.
 * Implements com.intellij.mcpserver.McpTool interface.
 */
class FindReferencingCodeSnippetsMcpTool : McpTool by McpToolAdapter(FindReferencingCodeSnippetsTool())
