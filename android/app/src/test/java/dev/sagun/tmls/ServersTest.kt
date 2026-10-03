package dev.sagun.tmls

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class ServersTest {
    @Test fun normalizeAddsTheSchemeAndDropsTheTrailingSlash() {
        assertEquals("http://192.168.0.5:8794", ServerUrl.normalize(" 192.168.0.5:8794/ "))
        assertEquals("https://tmls.example.com", ServerUrl.normalize("https://tmls.example.com/"))
    }

    @Test fun normalizeRejectsWhatIsNotAnAddress() {
        assertNull(ServerUrl.normalize(""))
        assertNull(ServerUrl.normalize("   "))
        assertNull(ServerUrl.normalize("http://"))
        assertNull(ServerUrl.normalize("https:///"))
        assertNull(ServerUrl.normalize("my server"))
    }

    @Test fun nothingSavedUsesTheBuiltInDefaults() {
        // make.sh builds the owner's addresses in from a local file, so the app needs no setup
        assertEquals(listOf("http://lan:8794", "https://tmls.example.com"),
            Servers.effective(null, null, "http://lan:8794", "https://tmls.example.com"))
        assertEquals(listOf("http://lan:8794"), Servers.effective(null, null, "http://lan:8794", ""))
        assertEquals(emptyList<String>(), Servers.effective(null, null, "", ""))  // a build without them: Setup
    }

    @Test fun savedAddressesWinOverTheDefaults() {
        assertEquals(listOf("http://mine:1"), Servers.effective("http://mine:1", null, "http://lan:8794", "https://tmls.example.com"))
    }

    @Test fun homeComesFirstAndAnEmptyOrRepeatedAwayIsDropped() {
        assertEquals(listOf("http://a", "https://b"), Servers.order("http://a", "https://b"))
        assertEquals(listOf("http://a"), Servers.order("http://a", null))
        assertEquals(listOf("http://a"), Servers.order("http://a", "http://a"))
        assertEquals(emptyList<String>(), Servers.order(null, null))
    }
}
