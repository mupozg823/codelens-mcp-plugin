package com.codelens.tools.adapters

import com.codelens.tools.ListDirectoryTreeTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

class ListDirectoryTreeMcpTool : McpTool by McpToolAdapter(ListDirectoryTreeTool())
