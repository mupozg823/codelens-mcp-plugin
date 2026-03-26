package com.codelens.tools.adapters

import com.codelens.tools.FindFileTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

class FindFileMcpTool : McpTool by McpToolAdapter(FindFileTool())
