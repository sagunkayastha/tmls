package dev.sagun.tmls.ui

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class WebTargetTest {
    @Test fun aKeyCannotLeaveItsString() {
        val script = WebTarget.Session("pc/a\");alert(1);(\"").script()
        assertTrue(script.endsWith("""("pc/a\");alert(1);(\"")"""))
        assertTrue(WebTarget.New("local", "arch\nbox").script().endsWith("""("local","arch\nbox")"""))
    }

    @Test fun whatThePageAnswers() {
        assertEquals("pc/a \"b\"", WebTarget.string("\"pc/a \\\"b\\\"\""))
        assertNull(WebTarget.string("null"))
        assertNull(WebTarget.string(null))
        assertNull(WebTarget.string("true"))
        assertNull(WebTarget.string("\"broken"))
    }
}
