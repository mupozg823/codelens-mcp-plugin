package com.codelens.tools.adapters

import com.codelens.tools.GetCurrentConfigTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

class GetCurrentConfigMcpTool : McpTool by McpToolAdapter(GetCurrentConfigTool())
