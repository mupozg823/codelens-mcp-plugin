package com.codelens.tools.adapters

import com.codelens.tools.GetProjectModulesTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

class GetProjectModulesMcpTool : McpTool by McpToolAdapter(GetProjectModulesTool())
