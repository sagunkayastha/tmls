package dev.sagun.tmls.chat

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class NewSessionTest {
    @Test fun defaultNameIsTheFoldersLastPartOrHome() {
        assertEquals(listOf("home", "home", "home", "tmls", "tmls", "x y"), listOf("~", "", "~/", "~/Others/tmls", "/a/tmls/", " /p/x y ").map(NewSession::defaultName))
    }

    @Test fun startsTheirLabelsAndTheDefault() {
        val starts = listOf(Start("shell", null), Start("claude", "claude"), Start("codex", "codex"), Start("cdsp", "claude --dangerously-skip-permissions"))
        assertEquals(listOf("Shell", "Claude", "Codex", "cdsp"), starts.map(NewSession::label))
        assertEquals(listOf(false, true, false, true), starts.map(NewSession::startsClaude))
        assertTrue(NewSession.startsClaude(Start("x", "/usr/local/bin/claude --model opus")))
        assertEquals("claude", NewSession.defaultStart(starts)?.id)
        assertEquals("shell", NewSession.defaultStart(listOf(Start("shell", null)))?.id)
        assertNull(NewSession.defaultStart(emptyList()))
    }

    @Test fun namesTmuxRefusesAreCaughtBeforeSending() {
        assertNull(NewSession.nameError(" work "))
        assertEquals("the session needs a name", NewSession.nameError("  "))
        assertEquals("names can't contain : or .", NewSession.nameError("a.b"))
        assertEquals("names can't contain #", NewSession.nameError("a#b"))
        assertEquals("names can't start with -", NewSession.nameError("-x"))
    }

    @Test fun folderChoicesStartAtHome() {
        assertEquals(listOf("~"), NewSession.choices(null))
        assertEquals(listOf("~", "~/a", "/srv"), NewSession.choices(Folders("/home/me", listOf("~/a", "~", "/srv"))))
        assertFalse(NewSession.choices(Folders("/h", listOf("~/a", "~/a"))).size > 2)
    }
}
