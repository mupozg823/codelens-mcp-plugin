package com.codelens.tools.adapters

import com.codelens.tools.ActivateProjectTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

class ActivateProjectMcpTool : McpTool by McpToolAdapter(ActivateProjectTool())
