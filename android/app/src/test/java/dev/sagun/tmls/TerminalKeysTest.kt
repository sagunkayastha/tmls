package dev.sagun.tmls

import dev.sagun.tmls.ui.TerminalKeys
import org.junit.Assert.assertEquals
import org.junit.Test

class TerminalKeysTest {
    @Test fun aGrowingWordSendsOnlyEachNewLetterOnce() {
        // what a composing keyboard sends while typing "hello": the whole word each time
        var composing = ""
        val out = StringBuilder()
        for (w in listOf("h", "he", "hel", "hell", "hello")) { out.append(TerminalKeys.change(composing, w)); composing = w }
        out.append(TerminalKeys.change(composing, "hello "))  // committed with a space
        assertEquals("hello ", out.toString())
    }

    @Test fun aCorrectionBacksUpOnlyWhatChanged() {
        assertEquals("\u007fat", TerminalKeys.change("thw", "that"))  // "th" kept, "w" -> "at"
    }

    @Test fun reSendingTheSameTextSendsNothing() {
        assertEquals("", TerminalKeys.change("bdiduebs.", "bdiduebs."))
    }

    @Test fun aNewlineIsEnter() {
        assertEquals("ls\r", TerminalKeys.change("", "ls\n"))
    }
}
