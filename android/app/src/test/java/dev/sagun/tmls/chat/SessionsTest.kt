package dev.sagun.tmls.chat

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class SessionsTest {
    private fun row(host: String, name: String, mark: String = "idle", background: Boolean = false) =
        Row("$host/$name", host, host.uppercase(), name, mark, background = background)

    private val hosts = listOf(
        HostInfo("local", "archbox", true, false), HostInfo("pc", "desk", true, false),
        HostInfo("nano", "nano", false, false), HostInfo("local#side", "side", true, true),
    )

    @Test fun fullReplacesAndDiffsApply() {
        var rows = Sessions.apply(emptyMap(), EventMsg.Rows(true, listOf(row("local", "a"), row("pc", "b")), emptyList()))
        rows = Sessions.apply(rows, EventMsg.Rows(false, listOf(row("pc", "b", "running"), row("pc", "c")), listOf("local/a")))
        assertEquals(listOf("pc/b", "pc/c"), rows.keys.toList())
        assertEquals("running", rows["pc/b"]?.mark)
        rows = Sessions.apply(rows, EventMsg.Rows(true, listOf(row("local", "z")), emptyList()))
        assertEquals(listOf("local/z"), rows.keys.toList())
    }

    @Test fun groupedByHostInTheServersOrderByName() {
        val groups = Sessions.groups(listOf(row("pc", "zeta"), row("local", "beta"), row("pc", "Alpha"), row("local", "alpha")), hosts)
        assertEquals(listOf("archbox", "desk"), groups.map { it.label })
        assertEquals(listOf("alpha", "beta"), groups[0].rows.map { it.name })
        assertEquals(listOf("Alpha", "zeta"), groups[1].rows.map { it.name })
    }

    @Test fun waitingComesFirst() {
        val groups = Sessions.groups(
            listOf(row("local", "a"), row("local", "b", "running"), row("pc", "m"), row("pc", "z", "waiting"), row("pc", "y", "waiting")), hosts,
        )
        assertEquals(listOf("desk", "archbox"), groups.map { it.label })
        assertEquals(listOf("y", "z", "m"), groups[0].rows.map { it.name })
    }

    @Test fun emptyAndBackgroundHostsAreLeftOutAndOfflineIsMarked() {
        val groups = Sessions.groups(listOf(row("nano", "n"), row("local#side", "s", background = true), row("ghost", "g")), hosts)
        assertEquals(listOf("nano", "GHOST"), groups.map { it.label })
        assertEquals(false, groups[0].online)
        assertTrue(Sessions.groups(emptyList(), hosts).isEmpty())
        assertEquals(listOf("local", "pc"), Sessions.creatable(hosts).map { it.host })
    }

    @Test fun marksOfTheWebPage() {
        assertEquals(listOf("●", "?", "◆", "✕", "○", ""), listOf("running", "waiting", "done", "failed", "idle", "new").map(Sessions::glyph))
    }

    @Test fun statesInAWordAndAgentsByName() {
        assertEquals(listOf("Running", "Waiting", "Idle", "Failed", "Idle"), listOf("running", "waiting", "done", "failed", "idle").map(Sessions::state))
        assertEquals(listOf("Claude", "Codex", "opencode", "Gemini", null, null), listOf("claude", "codex", "opencode", "gemini", "", null).map(Sessions::agentName))
    }

    @Test fun waitingSortsFirstWhateverTheAgentAndTheRestKeepTheirOrder() {
        val rows = listOf(row("local", "a"), row("local", "b", "running").copy(agent = "codex"), row("local", "c", "waiting").copy(agent = "codex"),
            row("local", "d", "waiting"), row("local", "e", "done"))
        assertEquals(listOf("c", "d", "a", "b", "e"), Sessions.groups(rows, hosts).single().rows.map { it.name })
    }
}
