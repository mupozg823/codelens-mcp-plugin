package com.codelens.tools.adapters

import com.codelens.tools.ListDirTool
import com.intellij.mcpserver.McpTool

class ListDirMcpTool : McpTool by McpToolAdapter(ListDirTool())
