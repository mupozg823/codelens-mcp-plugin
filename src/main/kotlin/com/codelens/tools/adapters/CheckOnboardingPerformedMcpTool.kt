package com.codelens.tools.adapters

import com.codelens.tools.CheckOnboardingPerformedTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

class CheckOnboardingPerformedMcpTool : McpTool by McpToolAdapter(CheckOnboardingPerformedTool())
