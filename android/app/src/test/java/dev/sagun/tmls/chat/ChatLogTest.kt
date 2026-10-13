package dev.sagun.tmls.chat

import java.time.ZoneId
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

class ChatLogTest {
    private fun user(id: String, ts: Long = 100, text: String = "q") = ChatItem(id, ts, Kind.USER, text = text)
    private fun reply(id: String, ts: Long = 100, text: String = "a") = ChatItem(id, ts, Kind.ASSISTANT, text = text)
    private fun tool(id: String, ts: Long = 100, tool: String = "Bash", output: String? = null) =
        ChatItem(id, ts, Kind.TOOL, tool = tool, summary = "Ran $id", input = id, output = output)

    @Test fun resetReplacesAndAppendAdds() {
        var log = ChatLog().apply(ChatMsg.Items(true, listOf(user("u1"), reply("a1"))))
        log = log.apply(ChatMsg.Items(false, listOf(tool("t1"))))
        assertEquals(listOf("u1", "a1", "t1"), log.items.map { it.id })
        log = log.apply(ChatMsg.Items(true, listOf(user("u9"))))
        assertEquals(listOf("u9"), log.items.map { it.id })
        log = log.apply(ChatMsg.Items(true, emptyList()))  // a Claude session nothing was typed in yet
        assertTrue(log.items.isEmpty())
    }

    @Test fun anIdSentAgainIsUpdatedWhereItStands() {
        var log = ChatLog().apply(ChatMsg.Items(true, listOf(user("u1"), reply("a1", text = "one"), tool("t1"))))
        log = log.apply(ChatMsg.Items(false, listOf(reply("a1", text = "two"), reply("a2"))))
        assertEquals(listOf("u1", "a1", "t1", "a2"), log.items.map { it.id })
        assertEquals("two", log.items[1].text)
        // Twice in one message: one row, or the screen's list would have two rows with one key.
        log = ChatLog().apply(ChatMsg.Items(true, listOf(user("u1", text = "x"), user("u1", text = "y"))))
        assertEquals(listOf("y"), log.items.map { it.text })
    }

    @Test fun patchFillsAToolInPlace() {
        val start = ChatLog().apply(ChatMsg.Items(true, listOf(tool("t1"), tool("t2"))))
        val log = start.apply(ChatMsg.Patch("t1", "done", true, listOf(Blob("b", "image/png"))))
        assertEquals("done", log.items[0].output)
        assertTrue(log.items[0].error)
        assertEquals(listOf(Blob("b", "image/png")), log.items[0].images)
        assertEquals(null, log.items[1].output)
        assertEquals("Ran t1", log.items[0].summary)
        // A result of only pictures: the empty string, which is not "still running".
        assertEquals("", start.apply(ChatMsg.Patch("t2", "", false, emptyList())).items[1].output)
        assertSame(start, start.apply(ChatMsg.Patch("nope", "x", false, emptyList())))
    }

    @Test fun theListStopsGrowing() {
        var log = ChatLog()
        repeat(12) { n -> log = log.apply(ChatMsg.Items(false, (0 until 100).map { reply("a$n-$it") })) }
        assertEquals(ChatLog.MAX, log.items.size)
        assertEquals("a11-99", log.items.last().id)
    }

    @Test fun consecutiveToolsFoldIntoOneRow() {
        val rows = ChatRows.build(listOf(user("u1"), tool("t1"), tool("t2"), tool("t3"), reply("a1"), tool("t4"), reply("a2")))
        assertEquals(listOf("i:u1", "g:t1", "i:a1", "i:t4", "i:a2"), rows.map { it.key })
        assertEquals("Ran 3 commands", (rows[1] as ChatRow.Tools).label)
        assertTrue(rows[3] is ChatRow.Tool)
        // The group keeps its key as it grows: the list does not rebuild it.
        val more = ChatRows.build(listOf(user("u1"), tool("t1"), tool("t2"), tool("t3"), tool("t3b")))
        assertEquals("g:t1", more[1].key)
        assertEquals("Used 2 tools", ChatRows.groupLabel(listOf(tool("a"), tool("b", tool = "Read"))))
    }

    @Test fun aLongPauseGetsATimeDivider() {
        val rows = ChatRows.build(listOf(user("u1", 1000), tool("t1", 1010), tool("t2", 1010 + ChatRows.GAP_S), reply("a1", 1700), user("u2", 1700 + 3600)))
        assertEquals(listOf("i:u1", "i:t1", "t:t2", "i:t2", "i:a1", "t:u2", "i:u2"), rows.map { it.key })
        assertEquals(1010 + ChatRows.GAP_S, (rows[2] as ChatRow.Time).ts)
        // An item without a time neither gets a divider nor moves the clock.
        assertEquals(listOf("i:u1", "i:a0", "i:a1"), ChatRows.build(listOf(user("u1", 1000), reply("a0", 0), reply("a1", 1100))).map { it.key })
    }

    @Test fun emptyTextIsNoRowAndKeysAreUnique() {
        val rows = ChatRows.build(listOf(user("u1", text = " "), reply("a1", text = ""), ChatItem("n", 1, Kind.NOTE, text = "Interrupted"),
            ChatItem("u2", 1, Kind.USER, images = listOf(Blob("b", "image/png")))))
        assertEquals(listOf("i:n", "i:u2"), rows.map { it.key })
        val many = ChatRows.build((0 until 300).map { if (it % 3 == 0) user("u$it", it * 400L + 1) else tool("t$it", it * 400L + 1) })
        assertEquals(many.size, many.map { it.key }.toSet().size)
    }

    @Test fun timeLabels() {
        val zone = ZoneId.of("UTC")
        val now = 1_760_313_600L  // Mon 2025-10-13 00:00 UTC
        assertEquals("00:00", ChatRows.timeLabel(now, now + 120, zone))
        assertEquals("Sunday 23:52", ChatRows.timeLabel(now - 480, now + 120, zone))
        assertEquals("3 Oct 14:10", ChatRows.timeLabel(now - 10 * 86400 + 14 * 3600 + 600, now, zone))
        assertEquals("13 Oct 2024 00:00", ChatRows.timeLabel(now - 365 * 86400, now, zone))
    }

    @Test fun headOfALongText() {
        assertEquals("short" to false, ChatRows.head("short"))
        assertEquals("a\nb\n" to false, ChatRows.head("a\nb\n", lines = 5))
        assertEquals("a\nb" to true, ChatRows.head("a\nb\nc", lines = 2))
        val (head, more) = ChatRows.head("x".repeat(5000), chars = 3000)
        assertEquals(3000, head.length)
        assertTrue(more)
        assertFalse(ChatRows.head("x".repeat(3000), chars = 3000).second)
    }
}
