package dev.sagun.tmls

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class OriginsTest {
    @Test fun theServerAndItsSketchpadAreTrusted() {
        // sketchpad is framed from the same host on another port, or a sibling name
        assertTrue(Origins.trusted("http://192.168.1.10:8790", "http://192.168.1.10:8794"))
        assertTrue(Origins.trusted("https://sketchpad.example.com", "https://tmls.example.com"))
        assertTrue(Origins.trusted("https://tmls.example.com", "https://tmls.example.com"))
    }

    @Test fun anythingElseIsNot() {
        assertFalse(Origins.trusted("https://evil.com", "https://tmls.example.com"))
        assertFalse(Origins.trusted("https://example.com", "https://tmls.example.com"))  // the bare parent: not a sibling
        assertFalse(Origins.trusted("http://192.168.1.11:8790", "http://192.168.1.10:8794"))
        assertFalse(Origins.trusted("http://10.168.1.10:8790", "http://192.168.1.10:8794"))  // IPs: exact only
        assertFalse(Origins.trusted("https://a.b.evil.com", "https://tmls.example.com"))
        assertFalse(Origins.trusted("null", "https://tmls.example.com"))
        assertFalse(Origins.trusted("", "https://tmls.example.com"))
    }
}

class ScreenshotsTest {
    @Test fun aBigScreenshotShrinksToFitKeepingItsShape() {
        org.junit.Assert.assertEquals(900 to 2000, Screenshots.scaled(1080, 2400))
        org.junit.Assert.assertEquals(2000 to 900, Screenshots.scaled(2400, 1080))
    }

    @Test fun aSmallOneIsLeftAlone() {
        org.junit.Assert.assertEquals(800 to 600, Screenshots.scaled(800, 600))
    }
}
