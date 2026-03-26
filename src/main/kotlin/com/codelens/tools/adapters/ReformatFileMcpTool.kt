package com.codelens.tools.adapters

import com.codelens.tools.McpToolAdapter
import com.codelens.tools.ReformatFileTool
import com.intellij.mcpserver.McpTool

class ReformatFileMcpTool : McpTool by McpToolAdapter(ReformatFileTool())
