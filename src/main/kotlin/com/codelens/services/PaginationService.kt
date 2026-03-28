package com.codelens.services

import java.util.LinkedHashMap
import java.util.UUID

/**
 * Cursor-based pagination service for large result sets.
 * Stores result pages with LRU eviction and TTL expiry.
 *
 * Inspired by jetbrains-index-mcp-plugin's PaginationService pattern.
 */
class PaginationService(
    private val maxCursors: Int = 20,
    private val ttlMs: Long = 10 * 60 * 1000 // 10 minutes
) {
    private val cursors = object : LinkedHashMap<String, CursorEntry<*>>(maxCursors, 0.75f, true) {
        override fun removeEldestEntry(eldest: MutableMap.MutableEntry<String, CursorEntry<*>>): Boolean {
            return size > maxCursors
        }
    }

    /**
     * Store results and return a paginated response.
     *
     * @param items Full list of results
     * @param pageSize Number of items per page
     * @return First page + cursor for next page (if more results exist)
     */
    fun <T> paginate(items: List<T>, pageSize: Int = 20): PaginatedResult<T> {
        evictExpired()

        if (items.size <= pageSize) {
            return PaginatedResult(
                items = items,
                totalCount = items.size,
                hasMore = false,
                cursor = null
            )
        }

        val cursorId = UUID.randomUUID().toString().substring(0, 8)
        cursors[cursorId] = CursorEntry(
            items = items,
            offset = pageSize,
            createdAt = System.currentTimeMillis()
        )

        return PaginatedResult(
            items = items.subList(0, pageSize),
            totalCount = items.size,
            hasMore = true,
            cursor = cursorId
        )
    }

    /**
     * Fetch the next page using a cursor.
     *
     * @param cursorId Cursor from a previous paginated response
     * @param pageSize Number of items per page
     * @return Next page + cursor (null if no more pages)
     */
    @Suppress("UNCHECKED_CAST")
    fun <T> fetchNext(cursorId: String, pageSize: Int = 20): PaginatedResult<T>? {
        evictExpired()

        val entry = cursors[cursorId] as? CursorEntry<T> ?: return null
        val items = entry.items
        val offset = entry.offset

        if (offset >= items.size) {
            cursors.remove(cursorId)
            return PaginatedResult(
                items = emptyList(),
                totalCount = items.size,
                hasMore = false,
                cursor = null
            )
        }

        val end = minOf(offset + pageSize, items.size)
        val page = items.subList(offset, end)
        val hasMore = end < items.size

        if (hasMore) {
            entry.offset = end
        } else {
            cursors.remove(cursorId)
        }

        return PaginatedResult(
            items = page,
            totalCount = items.size,
            hasMore = hasMore,
            cursor = if (hasMore) cursorId else null
        )
    }

    /**
     * Invalidate all cursors (e.g., on PSI change).
     */
    fun invalidateAll() {
        cursors.clear()
    }

    private fun evictExpired() {
        val now = System.currentTimeMillis()
        cursors.entries.removeIf { now - it.value.createdAt > ttlMs }
    }

    private data class CursorEntry<T>(
        val items: List<T>,
        var offset: Int,
        val createdAt: Long
    )
}

data class PaginatedResult<T>(
    val items: List<T>,
    val totalCount: Int,
    val hasMore: Boolean,
    val cursor: String?
) {
    fun toMap(): Map<String, Any?> = buildMap {
        put("total_count", totalCount)
        put("has_more", hasMore)
        if (cursor != null) put("cursor", cursor)
    }
}
