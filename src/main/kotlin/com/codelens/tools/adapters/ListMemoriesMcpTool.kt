package com.codelens.tools.adapters

import com.codelens.tools.ListMemoriesTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

class ListMemoriesMcpTool : McpTool by McpToolAdapter(ListMemoriesTool())
