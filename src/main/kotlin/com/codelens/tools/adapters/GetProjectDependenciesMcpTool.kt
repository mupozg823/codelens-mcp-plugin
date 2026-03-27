package com.codelens.tools.adapters

import com.codelens.tools.GetProjectDependenciesTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

class GetProjectDependenciesMcpTool : McpTool by McpToolAdapter(GetProjectDependenciesTool())
