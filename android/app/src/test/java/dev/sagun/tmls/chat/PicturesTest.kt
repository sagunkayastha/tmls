package dev.sagun.tmls.chat

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class PicturesTest {
    @Test fun findsAbsoluteAndHomePathsToPictures() {
        val text = "Saved `/data/scratch/x/shots/s19.png` and ~/foo/bar.jpg, also /a/B.JPEG. And /x/y.webp; /q.gif"
        assertEquals(listOf("/data/scratch/x/shots/s19.png", "~/foo/bar.jpg", "/a/B.JPEG", "/x/y.webp", "/q.gif"), ImagePaths.find(text))
    }

    @Test fun leavesAloneWhatIsNotAPicturePath() {
        val text = "https://example.com/a.png www/b.png relative/c.png ./d.png /e.png.bak /f.png/g /h.svg /i.pngx ~user/j.png " +
            "/data/notes.txt file:///k.png"
        assertTrue(ImagePaths.find(text).toString(), ImagePaths.find(text).isEmpty())
    }

    @Test fun eachPathOnceInBracketsAndLists() {
        val text = "- (/a/one.png)\n- [/a/two.jpg]\n- /a/one.png again\n- \"/a/three.gif\""
        assertEquals(listOf("/a/one.png", "/a/two.jpg", "/a/three.gif"), ImagePaths.find(text))
    }

    @Test fun namesAndKept() {
        assertEquals("s19.png", ImagePaths.name("/data/x/s19.png"))
        assertTrue(ImagePaths.isImageName("/x/A.JPG") && !ImagePaths.isImageName("/x/a.pdf"))
        assertTrue(ImagePaths.isKept("/home/u/Documents/tmls/2026-10/RAM.jpg"))
        assertFalse(ImagePaths.isKept("/home/u/.cache/tmls/images/x.jpg"))
    }

    @Test fun attachedAreUploadsAndKeptOnly() {
        val up = "/home/u/.cache/tmls/images/20261015-021500-0a1b2c3d-RAM.jpg"
        val kept = "/home/u/Documents/tmls/2026-10/RAM-1.png"
        assertEquals(listOf(up, kept), ImagePaths.attached("see $up and $kept, not /data/plot.png or $up"))
    }

    @Test fun sentTextLosesImageTokensAndPicturePaths() {
        assertEquals("what is this?", SentText.clean("[Image #1] what is this?", emptyList()))
        assertEquals("", SentText.clean("[Image #1][Image #2]", emptyList()))
        assertEquals("Here are my plots\n\nand the kept one",
            SentText.clean("<pasted_content id=\"51f6\">\nHere are my plots\n\nand the kept one\n</pasted_content id=\"51f6\">", emptyList()))
        val path = "/home/u/.cache/tmls/images/20261015-021500-0a1b2c3d-RAM.jpg"
        assertEquals("look at please", SentText.clean("look at $path please", listOf(path)))
        assertEquals("two:\n\nok", SentText.clean("two:\n$path\n\n\n\nok", listOf(path)))
        assertEquals("plain text stays", SentText.clean("plain text stays", emptyList()))
    }

    @Test fun sniffsByMagicBytes() {
        fun b(vararg x: Int) = ByteArray(x.size) { x[it].toByte() }
        assertEquals("image/png", PictureBytes.sniff(b(0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0)))
        assertEquals("image/jpeg", PictureBytes.sniff(b(0xFF, 0xD8, 0xFF, 0xE0)))
        assertEquals("image/gif", PictureBytes.sniff("GIF89a....".toByteArray()))
        assertEquals("image/webp", PictureBytes.sniff("RIFF\u0000\u0000\u0000\u0000WEBPVP8 ".toByteArray()))
        assertNull(PictureBytes.sniff("<svg>".toByteArray()))
        assertNull(PictureBytes.sniff(ByteArray(0)))
    }

    @Test fun fileNamesFitTheirType() {
        assertEquals("s19.png", PictureBytes.fileName("s19.png", "image/png"))
        assertEquals("photo.jpeg", PictureBytes.fileName("photo.jpeg", "image/jpeg"))
        assertEquals("plot.gif", PictureBytes.fileName("plot.png", "image/gif"))
        assertEquals("photo-20261015.jpg", PictureBytes.fileName("photo-20261015", "image/jpeg"))
        assertEquals("a_b.png", PictureBytes.fileName("a b.png", "image/png"))
        assertEquals("picture", PictureBytes.fileName("...", null))
    }
}
