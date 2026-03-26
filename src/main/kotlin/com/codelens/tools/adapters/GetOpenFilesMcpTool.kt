package com.codelens.tools.adapters

import com.codelens.tools.GetOpenFilesTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

class GetOpenFilesMcpTool : McpTool by McpToolAdapter(GetOpenFilesTool())
