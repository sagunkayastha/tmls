package dev.sagun.tmls

import java.net.ServerSocket
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.runBlocking
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ReachTest {
    private val servers = mutableListOf<MockWebServer>()
    private val client = Reach.client()

    private fun server(vararg responses: MockResponse): String {
        val s = MockWebServer()
        responses.forEach(s::enqueue)
        s.start()
        servers += s
        return s.url("/").toString().trimEnd('/')
    }

    /** A port nothing listens on: connecting is refused at once. */
    private fun deadUrl(): String = ServerSocket(0).use { "http://127.0.0.1:${it.localPort}" }

    @After fun stop() = servers.forEach { it.shutdown() }

    @Test fun skipsADeadHomeAndFindsAway() = runBlocking {
        val away = server(MockResponse().setBody("ok"))
        assertEquals(away, Reach.first(listOf(deadUrl(), away), client))
        assertEquals("/healthz", servers[0].takeRequest().path)
    }

    @Test fun homeWinsWhenBothAnswer() = runBlocking {
        val home = server(MockResponse().setBody("ok").setBodyDelay(200, TimeUnit.MILLISECONDS))
        val away = server(MockResponse().setBody("ok"))
        assertEquals(home, Reach.first(listOf(home, away), client))
    }

    @Test fun aSlowHomeDoesNotHoldUpAway() = runBlocking {
        val home = server(MockResponse().setBody("ok").setHeadersDelay(5, TimeUnit.SECONDS))
        val away = server(MockResponse().setBody("ok"))
        val started = System.nanoTime()
        assertEquals(away, Reach.first(listOf(home, away), client))
        val ms = (System.nanoTime() - started) / 1_000_000
        assertTrue("took $ms ms", ms < 2000)
    }

    @Test fun anErrorStatusIsNotAnAnswer() = runBlocking {
        val home = server(MockResponse().setResponseCode(502))
        assertNull(Reach.first(listOf(home, deadUrl()), client))
    }

    @Test fun nothingToTryIsNull() = runBlocking {
        assertNull(Reach.first(emptyList(), client))
    }
}
