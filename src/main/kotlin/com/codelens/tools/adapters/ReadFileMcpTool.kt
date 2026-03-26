package com.codelens.tools.adapters

import com.codelens.tools.ReadFileTool
import com.intellij.mcpserver.McpTool

class ReadFileMcpTool : McpTool by McpToolAdapter(ReadFileTool())
