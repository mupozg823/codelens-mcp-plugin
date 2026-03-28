package com.codelens.tools

/**
 * Structured MCP error hierarchy with JSON-RPC error codes.
 * Tools throw these; McpProtocolHandler catches and maps to proper error responses.
 */
sealed class McpException(message: String, val code: Int) : Exception(message) {
    /** Required parameter missing or invalid */
    class InvalidParams(message: String) : McpException(message, -32602)

    /** Symbol, file, or resource not found */
    class NotFound(resource: String) : McpException("$resource not found", -32602)

    /** IDE is indexing, indexes not available */
    class IndexNotReady : McpException("IDE is in dumb mode — indexes not yet available. Retry shortly.", -32603)

    /** Tool is disabled in settings */
    class ToolDisabled(toolName: String) : McpException("Tool '$toolName' is disabled in settings", -32601)

    /** Internal error during tool execution */
    class InternalError(cause: Throwable) : McpException("Internal error: ${cause.message}", -32603)
}
