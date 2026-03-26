package com.codelens.tools.adapters

import com.codelens.tools.ExecuteTerminalCommandTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

class ExecuteTerminalCommandMcpTool : McpTool by McpToolAdapter(ExecuteTerminalCommandTool())
