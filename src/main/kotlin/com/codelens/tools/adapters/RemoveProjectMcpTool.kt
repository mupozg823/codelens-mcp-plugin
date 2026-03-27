package com.codelens.tools.adapters

import com.codelens.tools.RemoveProjectTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

class RemoveProjectMcpTool : McpTool by McpToolAdapter(RemoveProjectTool())
