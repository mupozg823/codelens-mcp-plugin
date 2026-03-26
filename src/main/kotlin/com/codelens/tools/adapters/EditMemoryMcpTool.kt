package com.codelens.tools.adapters

import com.codelens.tools.EditMemoryTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

class EditMemoryMcpTool : McpTool by McpToolAdapter(EditMemoryTool())
