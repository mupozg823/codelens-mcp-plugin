package com.codelens.tools.adapters

import com.codelens.tools.McpToolAdapter
import com.codelens.tools.RenameMemoryTool
import com.intellij.mcpserver.McpTool

class RenameMemoryMcpTool : McpTool by McpToolAdapter(RenameMemoryTool())
