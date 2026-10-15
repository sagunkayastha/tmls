package dev.sagun.tmls.chat

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class MediaTest {
    private val a = "/home/u/.cache/tmls/images/20261015-021500-0a1b2c3d-RAM.jpg"
    private val b = "/home/u/.cache/tmls/images/20261015-021501-0a1b2c3e-notes.pdf"

    @Test fun onlyFilesStillNamedInTheMessageAreShown() {
        val list = listOf(Attached(a, "RAM.jpg"), Attached(b, "notes.pdf"))
        assertEquals(listOf("RAM.jpg"), Attachments.shown(list, "look at $a please").map { it.name })
        assertTrue(Attachments.shown(null, a).isEmpty())
    }

    @Test fun keepTogglesAndAKeptOneStaysKept() {
        var list = listOf(Attached(a, "RAM.jpg"), Attached(b, "notes.pdf"))
        list = Attachments.toggle(list, a)
        assertEquals(listOf(true, false), list.map { it.keep })
        list = Attachments.toggle(list, a)
        assertEquals(listOf(false, false), list.map { it.keep })
        list = Attachments.keptAs(Attachments.toggle(list, b), b, "/home/u/Documents/tmls/2026-10/notes.pdf")
        assertEquals(Attached("/home/u/Documents/tmls/2026-10/notes.pdf", "notes.pdf", keep = true, kept = true), list[1])
        assertEquals(list, Attachments.toggle(list, "/home/u/Documents/tmls/2026-10/notes.pdf"))
    }

    @Test fun uploadPathsInSentMessages() {
        val text = "see $a and ~/.cache/tmls/images/20261012-101010-123456.png. Also $a again, not /home/u/.cache/tmls/x.png " +
            "or /home/u/Documents/tmls/2026-10/RAM.jpg"
        assertEquals(listOf(a, "~/.cache/tmls/images/20261012-101010-123456.png"), Uploads.inText(text))
        assertEquals(listOf("/data/h/.cache/tmls/images/20261015-021500-0a1b2c3d-Bérénice_写真.png"),
            Uploads.inText("(/data/h/.cache/tmls/images/20261015-021500-0a1b2c3d-Bérénice_写真.png)"))
        assertTrue(Uploads.inText("nothing here").isEmpty())
    }

    @Test fun namesAndShortPaths() {
        assertEquals("RAM.jpg", Uploads.displayName(a))
        assertEquals("20261012-101010-123456.png", Uploads.displayName("~/.cache/tmls/images/20261012-101010-123456.png"))
        assertEquals("~/Documents/tmls/2026-10/RAM.jpg", Uploads.short("/home/me/Documents/tmls/2026-10/RAM.jpg"))
        assertEquals("~/Documents/x", Uploads.short("/root/Documents/x"))
        assertEquals("/data/x/Documents/y", Uploads.short("/data/x/Documents/y"))
    }

    @Test fun sentPhotosAreKeptUnderWhenTheyWereSent() {
        val utc = java.time.ZoneId.of("UTC")
        assertEquals("photo-20261015-021500", Uploads.photoName(1792030500, zone = utc))
        assertEquals("photo-20261015-021500-2", Uploads.photoName(1792030500, 2, utc))
        assertEquals("blob:b1", Uploads.blobKey("b1"))
    }

    @Test fun keepBodies() {
        assertEquals("""{"key":"h/a","path":"/p"}""", Wire.keep("h/a", "/p"))
        assertEquals("""{"key":"h/a","path":"/p","name":"n.jpg"}""", Wire.keep("h/a", "/p", "n.jpg"))
        assertEquals("""{"key":"h/a","blob":"b1","name":"photo"}""", Wire.keepPicture("h/a", "b1", "photo"))
    }

    @Test fun sharedTextGoesAfterTheMessage() {
        val link = Share("https://example.com/a", "A page", emptyList())
        assertEquals("what is this?\nA page\nhttps://example.com/a", link.compose(" what is this? "))
        assertEquals("A page\nhttps://example.com/a", link.compose(""))
        assertEquals("Title in it: A page", Share("Title in it: A page", "A page", emptyList()).compose(""))
        assertEquals("just me", Share(null, null, emptyList()).compose("just me"))
        assertEquals("draft\nnew", Share.append("draft \n", "new"))
        assertEquals("new", Share.append("  ", "new"))
        assertEquals("draft", Share.append("draft", ""))
        assertTrue(Share(" ", null, emptyList()).empty)
    }

    @Test fun summaries() {
        assertEquals("a link", Share.summary(emptyList(), "https://x"))
        assertEquals("text", Share.summary(emptyList(), "hello"))
        assertEquals("1 image", Share.summary(listOf("image/png"), null))
        assertEquals("3 images", Share.summary(listOf("image/png", "image/jpeg", "image/webp"), null))
        assertEquals("2 files and text", Share.summary(listOf("image/png", "application/pdf"), "x"))
        assertEquals("1 file", Share.summary(listOf(null), " "))
    }

    @Test fun shareTargetsWaitingThenLastLookedAtThenBusy() {
        fun row(name: String, mark: String = "idle", claude: Boolean = true, online: Boolean = true) =
            Row("h/$name", "h", "H", name, mark, claude = claude, online = online)
        val groups = listOf(
            HostGroup("h", "H", true, listOf(row("a"), row("b", "running"), row("c"), row("shell", claude = false), row("w", "waiting"))),
            HostGroup("g", "G", false, listOf(row("off", online = false))),
        )
        assertEquals(listOf("w", "c", "b", "a"), Share.targets(groups, "h/c").map { it.name })
        assertEquals(listOf("w", "b", "a", "c"), Share.targets(groups, null).map { it.name })
    }
}
