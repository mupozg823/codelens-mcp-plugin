package com.codelens.tools.adapters

import com.codelens.tools.GetFileProblemsTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

class GetFileProblemsMcpTool : McpTool by McpToolAdapter(GetFileProblemsTool())
