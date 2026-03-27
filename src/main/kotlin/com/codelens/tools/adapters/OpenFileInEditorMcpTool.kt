package com.codelens.tools.adapters

import com.codelens.tools.OpenFileInEditorTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

class OpenFileInEditorMcpTool : McpTool by McpToolAdapter(OpenFileInEditorTool())
