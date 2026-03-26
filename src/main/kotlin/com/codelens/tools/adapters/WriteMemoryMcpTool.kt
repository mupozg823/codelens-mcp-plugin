package com.codelens.tools.adapters

import com.codelens.tools.McpToolAdapter
import com.codelens.tools.WriteMemoryTool
import com.intellij.mcpserver.McpTool

class WriteMemoryMcpTool : McpTool by McpToolAdapter(WriteMemoryTool())
