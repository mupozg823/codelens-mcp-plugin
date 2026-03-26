package com.codelens.tools.adapters

import com.codelens.tools.DeleteMemoryTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

class DeleteMemoryMcpTool : McpTool by McpToolAdapter(DeleteMemoryTool())
