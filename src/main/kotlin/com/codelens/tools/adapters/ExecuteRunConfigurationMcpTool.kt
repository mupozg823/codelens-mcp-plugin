package com.codelens.tools.adapters

import com.codelens.tools.ExecuteRunConfigurationTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

class ExecuteRunConfigurationMcpTool : McpTool by McpToolAdapter(ExecuteRunConfigurationTool())
