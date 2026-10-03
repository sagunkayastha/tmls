package dev.sagun.tmls.update

import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.IOException
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class UpdateRulesTest {
    private val sha = "0123456789abcdef".repeat(4)
    private fun body(code: String = "29800000", name: String = "\"0.1.97-abc\"", size: String = "19654638", digest: String = "\"$sha\"") =
        """{"versionCode":$code,"versionName":$name,"size":$size,"sha256":$digest}"""

    @Test fun parsesThePublishedManifest() {
        val m = UpdateRules.parse(body())
        assertNotNull(m)
        assertEquals(29800000L, m!!.versionCode)
        assertEquals("0.1.97-abc", m.versionName)
        assertEquals(19654638L, m.size)
        assertEquals(sha, m.sha256)
    }

    @Test fun ignoresFieldsAddedLater() {
        assertNotNull(UpdateRules.parse(body().dropLast(1) + ""","notes":"x"}"""))
    }

    @Test fun rejectsAnythingNotFullyUnderstood() {
        assertNull(UpdateRules.parse(null))
        assertNull(UpdateRules.parse(""))
        assertNull(UpdateRules.parse("<html>login</html>"))
        assertNull(UpdateRules.parse("""{"versionCode":1}"""))
        assertNull(UpdateRules.parse(body(code = "0")))
        assertNull(UpdateRules.parse(body(size = "0")))
        assertNull(UpdateRules.parse(body(size = "${UpdateRules.MAX_APK_BYTES + 1}")))
        assertNull(UpdateRules.parse(body(digest = "\"abc\"")))
        assertNull(UpdateRules.parse(body(digest = "\"${sha.uppercase()}\"")))
        assertNull(UpdateRules.parse(body(name = "\"\"")))
        assertNull(UpdateRules.parse(body() + " ".repeat(UpdateRules.MAX_BODY)))
    }

    @Test fun onlyAStrictlyHigherCodeIsNewer() {
        assertTrue(UpdateRules.isNewer(installed = 10, available = 11))
        assertFalse(UpdateRules.isNewer(installed = 11, available = 11))
        assertFalse(UpdateRules.isNewer(installed = 12, available = 11))
    }

    @Test fun digestCompareIgnoresCase() {
        assertTrue(UpdateRules.digestMatches(sha, sha.uppercase()))
        assertFalse(UpdateRules.digestMatches(sha, sha.replace('0', '1')))
    }

    @Test fun hexIsLowercase() {
        assertEquals("00ff10", UpdateRules.hex(byteArrayOf(0, -1, 16)))
    }

    @Test fun copyReportsProgressCappedAt99() {
        val seen = mutableListOf<Int>()
        val out = ByteArrayOutputStream()
        val n = UpdateRules.copy(ByteArrayInputStream(ByteArray(200_000)), out, 200_000) { seen += it }
        assertEquals(200_000L, n)
        assertEquals(200_000, out.size())
        assertTrue(seen.isNotEmpty() && seen.all { it in 0..99 } && seen == seen.sorted().distinct())
    }

    @Test(expected = IOException::class) fun copyRefusesAShortBody() {
        UpdateRules.copy(ByteArrayInputStream(ByteArray(10)), ByteArrayOutputStream(), 11) {}
    }

    @Test(expected = IOException::class) fun copyRefusesALongBody() {
        UpdateRules.copy(ByteArrayInputStream(ByteArray(12)), ByteArrayOutputStream(), 11) {}
    }
}
