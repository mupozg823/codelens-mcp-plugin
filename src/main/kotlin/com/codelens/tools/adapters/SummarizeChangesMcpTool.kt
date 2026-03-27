package com.codelens.tools.adapters

import com.codelens.tools.SummarizeChangesTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

class SummarizeChangesMcpTool : McpTool by McpToolAdapter(SummarizeChangesTool())
