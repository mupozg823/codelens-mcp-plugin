package com.codelens.tools.adapters

import com.codelens.tools.GetRunConfigurationsTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

class GetRunConfigurationsMcpTool : McpTool by McpToolAdapter(GetRunConfigurationsTool())
