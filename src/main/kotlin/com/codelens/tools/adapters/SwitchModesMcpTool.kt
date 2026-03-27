package com.codelens.tools.adapters

import com.codelens.tools.SwitchModesTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

class SwitchModesMcpTool : McpTool by McpToolAdapter(SwitchModesTool())
