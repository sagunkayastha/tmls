package dev.sagun.tmls.chat

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class WireTest {
    @Test fun rowsWithTheChatFields() {
        val msg = Wire.event(
            """{"t":"rows","full":true,"gone":["a/old"],"set":[{"key":"local/web","host":"local","label":"archbox",
               "name":"web","mark":"waiting","line":"Bash: rm -rf x","waiting":"Bash","shown":["Do you want to proceed?","1. Yes"],
               "pane":"%7","viewers":[["tty1","80x24"]],"online":true,"background":false,"claude":true,"context":42,"model":"opus"}]}"""
        ) as EventMsg.Rows
        assertTrue(msg.full)
        assertEquals(listOf("a/old"), msg.gone)
        val row = msg.set.single()
        assertEquals("local/web", row.key)
        assertEquals("archbox", row.label)
        assertEquals(listOf("Do you want to proceed?", "1. Yes"), row.prompt)
        assertEquals("%7", row.pane)
        assertTrue(row.claude)
        assertEquals(42, row.context)
        assertEquals("opus", row.model)
    }

    @Test fun aPromptsChoicesAndTheApprovalThatSendsOne() {
        val row = (Wire.event(
            """{"t":"rows","set":[{"key":"a/b","host":"a","name":"b","mark":"waiting","waiting":"Bash","shown":["x"],
               "choices":[{"n":1,"label":"Yes"},{"n":2,"label":"Yes, always"},{"n":4,"label":"No"},{"n":"x"}],"pane":"%7"}],"gone":[]}"""
        ) as EventMsg.Rows).set.single()
        assertEquals(listOf(Choice(1, "Yes"), Choice(2, "Yes, always"), Choice(4, "No")), row.choices)
        assertEquals("Bash", row.waitingFor)
        assertTrue(Wire.approve(row, true, 2).contains("\"choice\":2"))
        assertFalse(Wire.approve(row, false).contains("choice"))
        val old = (Wire.event("""{"t":"rows","set":[{"key":"a/b","host":"a","name":"b","mark":"waiting","shown":["x"],"choices":null}],"gone":[]}""") as EventMsg.Rows).set.single()
        assertNull(old.choices)
    }

    @Test fun aRowOfAnOlderServerHasNoChat() {
        val msg = Wire.event("""{"t":"rows","set":[{"key":"pc/x","host":"pc","label":"pc","name":"x","mark":"idle","shown":null,"pane":null}],"gone":[]}""") as EventMsg.Rows
        val row = msg.set.single()
        assertFalse(msg.full)
        assertFalse(row.claude)
        assertNull(row.context)
        assertNull(row.prompt)
        assertNull(row.pane)
    }

    @Test fun aPromptOnlyCountsWhileWaiting() {
        val row = Row("a/b", "a", "a", "b", mark = "running", shown = listOf("x"))
        assertNull(row.prompt)
        assertTrue(row.working)
    }

    @Test fun otherEventsAreAliveButNothing() {
        assertEquals(EventMsg.Other, Wire.event("""{"t":"version","v":"abc"}"""))
        assertEquals(EventMsg.Renamed("a/b", "a/c"), Wire.event("""{"t":"renamed","old":"a/b","new":"a/c","name":"c"}"""))
        assertEquals(listOf(HostInfo("pc", "desk", false, false)), (Wire.event("""{"t":"hosts","hosts":[{"host":"pc","label":"desk","online":false,"background":false}]}""") as EventMsg.Hosts).hosts)
        assertNull(Wire.event("not json"))
        assertNull(Wire.event("[1,2]"))
        assertNull(Wire.event("""{"x":1}"""))
    }

    @Test fun chatOpenNoneItemsPatch() {
        assertEquals(ChatMsg.Open("h/n", "abc", null), Wire.chat("""{"t":"open","key":"h/n","session":"abc","title":null}"""))
        assertEquals(ChatMsg.Open("h/n", "abc", "Fix it"), Wire.chat("""{"t":"open","key":"h/n","session":"abc","title":"Fix it"}"""))
        assertEquals(ChatMsg.None("host unreachable"), Wire.chat("""{"t":"none","reason":"host unreachable"}"""))
        val items = Wire.chat(
            """{"t":"items","reset":true,"items":[
               {"id":"u1","ts":1760000000,"kind":"user","text":"hi","images":[{"id":"b1","mime":"image/png"}]},
               {"id":"a1","ts":1760000001,"kind":"assistant","text":"**yes**"},
               {"id":"t1","ts":1760000002,"kind":"tool","tool":"Bash","summary":"Ran ls -la","input":"ls -la","output":null,"error":false,"images":[]},
               {"id":"n1","ts":1760000003,"kind":"note","text":"Context compacted"},
               {"id":"x1","ts":1760000004,"kind":"hologram","text":"something new"},
               {"id":"x2","ts":1760000005,"kind":"hologram"},
               {"ts":1,"kind":"user","text":"no id"}]}"""
        ) as ChatMsg.Items
        assertTrue(items.reset)
        assertEquals(listOf("u1", "a1", "t1", "n1", "x1"), items.items.map { it.id })
        assertEquals(listOf(Blob("b1", "image/png")), items.items[0].images)
        assertEquals(Kind.TOOL, items.items[2].kind)
        assertEquals("Ran ls -la", items.items[2].summary)
        assertNull(items.items[2].output)
        assertEquals(Kind.NOTE, items.items[4].kind)  // an unknown kind with text: one grey line
        assertEquals(
            ChatMsg.Patch("t1", "", true, listOf(Blob("b2", "image/jpeg"))),
            Wire.chat("""{"t":"patch","id":"t1","output":"","error":true,"images":[{"id":"b2","mime":"image/jpeg"}]}"""),
        )
        assertNull(Wire.chat("""{"t":"later"}"""))
        assertNull(Wire.chat("{"))
    }

    @Test fun replies() {
        assertEquals(Reply(true), Wire.reply(200, """{"ok":true}"""))
        assertEquals(Reply(true, path = "/home/u/.cache/tmls/images/a.png"), Wire.reply(200, """{"ok":true,"path":"/home/u/.cache/tmls/images/a.png"}"""))
        assertEquals(Reply(false, "state changed"), Wire.reply(409, """{"ok":false,"error":"state changed"}"""))
        assertEquals(Reply(false, "HTTP 502"), Wire.reply(502, "<html>Bad Gateway</html>"))
        assertEquals(Reply(false, "HTTP 200"), Wire.reply(200, "<html>a proxy's page</html>"))
        assertTrue(Wire.reply(401, """{"ok":false,"error":"login"}""").login)
    }

    @Test fun aDialogIsUpOnlyWhenTheRowShowsOne() {
        assertTrue(Row("pc/x", "pc", "desk", "x", "waiting", shown = listOf("a")).asking)
        assertFalse(Row("pc/x", "pc", "desk", "x", "waiting").asking)
    }

    @Test fun denyIsOneOfTheChoicesOrNone() {
        assertEquals(2, denyAt(listOf(Choice(1, "Yes"), Choice(2, "Yes, always"), Choice(4, "No"))))
        assertEquals(1, denyAt(listOf(Choice(1, "Yes"), Choice(2, "No"), Choice(3, "No, and tell Claude"))))
        assertNull(denyAt(listOf(Choice(1, "Yes"), Choice(2, "Yes, always"))))
        assertNull(denyAt(listOf(Choice(1, "No way"))))  // the first choice is the Allow button
    }

    @Test fun bodies() {
        assertEquals("""{"key":"h/n","text":"a \"b\"\nc"}""", Wire.send("h/n", "a \"b\"\nc"))
        assertEquals("""{"key":"h/n","name":"Escape"}""", Wire.key("h/n", "Escape"))
        assertEquals("""{"key":"h/n","text":"x","session":"s1"}""", Wire.send("h/n", "x", "s1"))
        assertEquals("""{"key":"h/n","name":"Escape","session":"s1"}""", Wire.key("h/n", "Escape", "s1"))
        val row = Row("pc/x", "pc", "desk", "x", "waiting", shown = listOf("a", "b"), pane = "%3")
        assertEquals("""{"host":"pc","name":"x","shown":["a","b"],"yes":true,"pane":"%3"}""", Wire.approve(row, true))
        assertEquals("""{"host":"pc","name":"x","shown":["a","b"],"yes":false,"pane":null}""", Wire.approve(row.copy(pane = null), false))
    }

    @Test fun quoteIsAJavaScriptString() {
        assertEquals(""""a\"b\\c\n"""", Wire.quote("a\"b\\c\n"))
        assertEquals("\"x\\u2028y\"", Wire.quote("x y"))
    }
}
