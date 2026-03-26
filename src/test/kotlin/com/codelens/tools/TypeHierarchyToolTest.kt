package com.codelens.tools

import com.codelens.CodeLensTestBase

class TypeHierarchyToolTest : CodeLensTestBase() {

    fun testToolUsesSerenaCompatibleName() {
        assertEquals("get_type_hierarchy", TypeHierarchyTool().toolName)
    }

    fun testReportsJavaInheritance() {
        myFixture.addFileToProject("com/example/Base.java", """
            package com.example;

            public class Base {
                public void baseMethod() {}
            }
        """.trimIndent())
        myFixture.addFileToProject("com/example/Child.java", """
            package com.example;

            public class Child extends Base {}
        """.trimIndent())

        val response = TypeHierarchyTool().execute(
            mapOf("fully_qualified_name" to "com.example.Base"),
            project
        )

        assertTrue(response.contains("\"success\":true"))
        assertTrue(response.contains("\"class_name\":\"Base\""))
        assertTrue(response.contains("\"qualified_name\":\"com.example.Child\""))
    }

    fun testReportsKotlinDataClassKind() {
        myFixture.addFileToProject("com/example/Person.kt", """
            package com.example

            data class Person(val name: String)
        """.trimIndent())

        val response = TypeHierarchyTool().execute(
            mapOf("fully_qualified_name" to "com.example.Person"),
            project
        )

        assertTrue(response.contains("\"success\":true"))
        assertTrue(response.contains("\"kind\":\"data_class\""))
        assertTrue(response.contains("\"properties\":[\"name\"]"))
    }
}
