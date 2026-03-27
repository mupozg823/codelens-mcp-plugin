package com.codelens.tools.adapters

import com.codelens.tools.OnboardingTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

class OnboardingMcpTool : McpTool by McpToolAdapter(OnboardingTool())
