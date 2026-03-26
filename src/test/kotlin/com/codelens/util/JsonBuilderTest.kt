package com.codelens.util

import junit.framework.TestCase

class JsonBuilderTest : TestCase() {

    fun testToJsonNull() {
        assertEquals("null", JsonBuilder.toJson(null))
    }

    fun testToJsonString() {
        assertEquals("\"hello\"", JsonBuilder.toJson("hello"))
    }

    fun testToJsonStringEscaping() {
        val result = JsonBuilder.toJson("he said \"hi\"\nnewline")
        assertTrue("Should escape quotes", result.contains("\\\""))
        assertTrue("Should escape newlines", result.contains("\\n"))
    }

    fun testToJsonNumber() {
        assertEquals("42", JsonBuilder.toJson(42))
        assertEquals("3.14", JsonBuilder.toJson(3.14))
    }

    fun testToJsonBoolean() {
        assertEquals("true", JsonBuilder.toJson(true))
        assertEquals("false", JsonBuilder.toJson(false))
    }

    fun testToJsonList() {
        val result = JsonBuilder.toJson(listOf(1, 2, 3))
        assertEquals("[1,2,3]", result)
    }

    fun testToJsonMap() {
        val result = JsonBuilder.toJson(mapOf("key" to "value"))
        assertEquals("{\"key\":\"value\"}", result)
    }

    fun testToJsonNestedMap() {
        val result = JsonBuilder.toJson(mapOf("outer" to mapOf("inner" to 42)))
        assertTrue("Should contain nested structure", result.contains("\"inner\":42"))
    }

    fun testToJsonNullValuesFiltered() {
        val result = JsonBuilder.toJson(mapOf("a" to 1, "b" to null, "c" to 3))
        assertFalse("Null values should be filtered", result.contains("\"b\""))
    }

    fun testToolResponseSuccess() {
        val result = JsonBuilder.toolResponse(success = true, data = "test")
        assertTrue("Should contain success:true", result.contains("\"success\":true"))
        assertTrue("Should contain data", result.contains("\"data\":\"test\""))
    }

    fun testToolResponseError() {
        val result = JsonBuilder.toolResponse(success = false, error = "something broke")
        assertTrue("Should contain success:false", result.contains("\"success\":false"))
        assertTrue("Should contain error", result.contains("\"error\":\"something broke\""))
    }

    fun testToolResponseWithMetadata() {
        val meta = mapOf("tool" to "test_tool", "version" to 1)
        val result = JsonBuilder.toolResponse(success = true, metadata = meta)
        assertTrue("Should contain metadata", result.contains("\"metadata\""))
    }

    fun testSpecialCharactersInString() {
        val result = JsonBuilder.toJson("tab\there\rcarriage\bbackspace")
        assertTrue("Should escape tab", result.contains("\\t"))
        assertTrue("Should escape carriage return", result.contains("\\r"))
        assertTrue("Should escape backspace", result.contains("\\b"))
    }

    fun testEmptyMap() {
        assertEquals("{}", JsonBuilder.toJson(emptyMap<String, Any>()))
    }

    fun testEmptyList() {
        assertEquals("[]", JsonBuilder.toJson(emptyList<Any>()))
    }
}
