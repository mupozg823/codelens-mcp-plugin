package com.codelens.tools.adapters

import com.codelens.tools.TypeHierarchyTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

/**
 * MCP Tool adapter for get_type_hierarchy.
 * Implements com.intellij.mcpserver.McpTool interface.
 */
class TypeHierarchyMcpTool : McpTool by McpToolAdapter(TypeHierarchyTool())
