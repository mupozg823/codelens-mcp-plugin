package com.codelens.tools.adapters

import com.codelens.tools.InitialInstructionsTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

class InitialInstructionsMcpTool : McpTool by McpToolAdapter(InitialInstructionsTool())
