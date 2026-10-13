package dev.sagun.tmls.chat

import androidx.compose.ui.text.input.TextFieldValue
import org.junit.Assert.assertEquals
import org.junit.Test

class ChatUrlsTest {
    @Test fun originIsWhatABrowserOnTheSiteSends() {
        assertEquals("http://192.168.0.5:8794", ChatUrls("http://192.168.0.5:8794").origin)
        assertEquals("https://tmls.example.com", ChatUrls("https://tmls.example.com").origin)
        assertEquals("https://tmls.example.com", ChatUrls("https://tmls.example.com:443").origin)
        assertEquals("http://box", ChatUrls("http://box:80").origin)
        assertEquals("https://box:8443", ChatUrls("https://box:8443").origin)
        assertEquals("http://[::1]:8794", ChatUrls("http://[::1]:8794").origin)
    }

    @Test fun addresses() {
        val urls = ChatUrls("http://192.168.0.5:8794")
        assertEquals("http://192.168.0.5:8794/api/events", urls.events.toString())
        assertEquals("http://192.168.0.5:8794/api/chat/send", urls.post("api/chat/send").toString())
        assertEquals("http://192.168.0.5:8794/logout", urls.post("logout").toString())
    }

    @Test fun aKeySurvivesTheQuery() {
        val urls = ChatUrls("https://tmls.example.com")
        for (key in listOf("local/web", "pc#side/a b", "h/x&y=z", "h/a+b", "h/ü%41")) {
            assertEquals(key, urls.chat(key).queryParameter("key"))
            assertEquals("/api/chat", urls.chat(key).encodedPath)
        }
        val blob = urls.blob("local/web", "ab/c+d")
        assertEquals("/api/chat/blob", blob.encodedPath)
        assertEquals("local/web", blob.queryParameter("key"))
        assertEquals("ab/c+d", blob.queryParameter("id"))
        val upload = urls.upload("local/web", "my photo #1.jpg")
        assertEquals("my photo #1.jpg", upload.queryParameter("name"))
        assertEquals(null, upload.fragment)
        // The server splits a query at & and reads + as a space: neither may appear unescaped.
        assertEquals(false, urls.chat("h/a+b&c").encodedQuery!!.removePrefix("key=").any { it == '+' || it == '&' })
    }

    @Test fun backoffDoublesToItsLimitAndStartsOver() {
        val backoff = Backoff(first = 1_000, max = 5_000)
        assertEquals(listOf(1_000L, 2_000L, 4_000L, 5_000L, 5_000L), List(5) { backoff.next() })
        backoff.reset()
        assertEquals(1_000L, backoff.next())
    }

    @Test fun aPathGoesWhereTheCursorIs() {
        assertEquals("/a/b.png " to 9, Draft.insert("", 0, 0, "/a/b.png"))
        assertEquals("look at /a/b.png " to 17, Draft.insert("look at", 7, 7, "/a/b.png"))
        assertEquals("look /a/b.png at" to 14, Draft.insert("look at", 5, 5, "/a/b.png"))
        assertEquals("see /a/b.png here" to 12, Draft.insert("see THIS here", 4, 8, "/a/b.png"))
        assertEquals("see /a/b.png here" to 12, Draft.insert("see THIS here", 8, 4, "/a/b.png"))  // selected backwards
        assertEquals("x /p " to 5, Draft.insert("x", 99, 99, "/p"))
    }

    @Test fun fileNames() {
        assertEquals("a.png", Draft.fileName("a.png", "f"))
        assertEquals("c.txt", Draft.fileName("/a/b/c.txt", "f"))
        assertEquals("photo-1.jpg", Draft.fileName(null, "photo-1.jpg"))
        assertEquals("photo-1.jpg", Draft.fileName("  ", "photo-1.jpg"))
    }

    @Test fun eachSessionKeepsItsDraft() {
        val drafts = Drafts()
        assertEquals("", drafts.swap(null, TextFieldValue(""), "a").text)
        assertEquals("", drafts.swap("a", TextFieldValue("hello /x/y.png"), "b").text)
        assertEquals("hello /x/y.png", drafts.swap("b", TextFieldValue(""), "a").text)
        drafts.clear("a")
        assertEquals("", drafts.swap("b", TextFieldValue("more"), "a").text)
        assertEquals("more", drafts.swap("a", TextFieldValue(""), "b").text)
        drafts.insert("b", "/x/p.png")  // an upload that finished after the user left "b"
        drafts.insert("b", "/x/q.png")
        assertEquals("/x/p.png /x/q.png more", drafts.swap(null, TextFieldValue(""), "b").text)
    }
}
