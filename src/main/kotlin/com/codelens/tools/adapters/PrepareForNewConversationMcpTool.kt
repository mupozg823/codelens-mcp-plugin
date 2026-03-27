package com.codelens.tools.adapters

import com.codelens.tools.PrepareForNewConversationTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

class PrepareForNewConversationMcpTool : McpTool by McpToolAdapter(PrepareForNewConversationTool())
