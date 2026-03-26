package com.codelens.tools.adapters

import com.codelens.tools.McpToolAdapter
import com.codelens.tools.ReadMemoryTool
import com.intellij.mcpserver.McpTool

class ReadMemoryMcpTool : McpTool by McpToolAdapter(ReadMemoryTool())
