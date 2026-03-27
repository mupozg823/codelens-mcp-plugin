package com.codelens.tools.adapters

import com.codelens.tools.GetRepositoriesTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

class GetRepositoriesMcpTool : McpTool by McpToolAdapter(GetRepositoriesTool())
