package dev.sagun.tmls.chat

import java.io.ByteArrayInputStream
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.Executors
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withContext
import okhttp3.Cookie
import okhttp3.CookieJar
import okhttp3.HttpUrl
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import okhttp3.mockwebserver.Dispatcher
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import okhttp3.mockwebserver.RecordedRequest
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** The client against a real socket: a MockWebServer playing tmls-web. */
class ChatClientTest {
    private val server = MockWebServer()
    private val main = Executors.newSingleThreadExecutor()
    private val dispatcher = main.asCoroutineDispatcher()
    private val scope = CoroutineScope(dispatcher)
    private val seen = LinkedBlockingQueue<String>()
    private val requests = CopyOnWriteArrayList<RecordedRequest>()
    /** For each chat message: whether its slow half ran on the main thread (it must not). */
    private val prepared = CopyOnWriteArrayList<Boolean>()
    private val http = ChatClient.http().cookieJar(object : CookieJar {
        override fun loadForRequest(url: HttpUrl) = listOf(Cookie.Builder().name("tmls_session").value("abc").domain(url.host).build())
        override fun saveFromResponse(url: HttpUrl, cookies: List<Cookie>) {}
    }).build()

    private val sink = object : ChatClient.Sink {
        override fun onEvent(message: EventMsg) { seen += "event:$message" }
        override fun onFeed(up: Boolean) { seen += "feed:$up" }
        override fun onUnreachable() { seen += "unreachable" }
        override fun prepare(message: ChatMsg) { prepared += Thread.currentThread() == mainThread }
        override fun onChat(key: String, message: ChatMsg) { seen += "chat:$key:$message:${Thread.currentThread() == mainThread}" }
        override fun onChatLost(key: String) { seen += "chatlost:$key" }
        override fun onLogin() { seen += "login" }
        override fun onRefused(code: Int) { seen += "refused:$code" }
    }
    private val mainThread: Thread = main.submit<Thread> { Thread.currentThread() }.get()

    /** [routes]: path (without the query) to what the server answers, in turn; the last one repeats. */
    private fun serve(routes: Map<String, List<MockResponse>>) {
        val turn = HashMap<String, Int>()
        server.dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse {
                requests += request
                val path = request.requestUrl!!.encodedPath
                val answers = routes[path] ?: return MockResponse().setResponseCode(404)
                val n = synchronized(turn) { turn.merge(path, 1, Int::plus)!! - 1 }
                return answers[minOf(n, answers.lastIndex)]
            }
        }
        server.start()
    }

    private fun socket(vararg messages: String, close: Int? = null) = MockResponse().withWebSocketUpgrade(object : WebSocketListener() {
        override fun onOpen(webSocket: WebSocket, response: Response) {
            messages.forEach { webSocket.send(it) }
            if (close != null) webSocket.close(close, null)
        }

        override fun onClosing(webSocket: WebSocket, code: Int, reason: String) {
            webSocket.close(1000, null)
        }
    })

    private fun client() = ChatClient(http, server.url("/").toString().trimEnd('/'), scope, sink)

    private fun <T> onMain(block: suspend () -> T): T = runBlocking { withContext(dispatcher) { block() } }

    private fun next(): String = seen.poll(5, TimeUnit.SECONDS) ?: error("nothing arrived")

    private fun nextMatching(prefix: String): String {
        while (true) next().let { if (it.startsWith(prefix)) return it }
    }

    private fun count(path: String) = requests.count { it.requestUrl!!.encodedPath == path }

    @After fun stop() {
        scope.cancel()
        main.shutdownNow()
        http.dispatcher.cancelAll()
        http.connectionPool.evictAll()
        server.shutdown()
    }

    @Test fun theSessionListArrivesWithTheSitesOriginAndTheCookie() {
        serve(mapOf("/api/events" to listOf(socket("""{"t":"hosts","hosts":[]}""", """{"t":"rows","full":true,"set":[],"gone":[]}"""))))
        val client = client()
        onMain { client.start() }
        assertEquals("feed:true", next())
        assertEquals("event:Hosts(hosts=[])", next())
        assertEquals("feed:true", next())
        assertEquals("event:Rows(full=true, set=[], gone=[])", next())
        val request = requests.single()
        assertEquals("http://${server.hostName}:${server.port}", request.getHeader("Origin"))
        assertEquals("tmls_session=abc", request.getHeader("Cookie"))
        assertEquals("websocket", request.getHeader("Upgrade"))
        onMain { client.stop() }
    }

    @Test fun aLoginCloseIsNotRetried() {
        serve(mapOf("/api/events" to listOf(socket(close = Feed.LOGIN))))
        val client = client()
        onMain { client.start() }
        assertEquals("login", next())
        Thread.sleep(1_500)  // longer than the first reconnect wait
        assertEquals(1, count("/api/events"))
        assertNull(seen.poll())
    }

    @Test fun aRefusedOriginIsNotRetried() {
        serve(mapOf("/api/events" to listOf(socket(close = Feed.REFUSED))))
        val client = client()
        onMain { client.start() }
        assertEquals("refused:4403", next())
    }

    @Test fun aDroppedFeedIsOpenedAgainAndStopEndsIt() {
        serve(mapOf("/api/events" to listOf(socket("""{"t":"version","v":"1"}""", close = 1001), socket("""{"t":"killed","key":"a/b"}"""))))
        val client = client()
        onMain { client.start() }
        assertEquals("feed:true", next())
        assertEquals("event:Other", next())
        assertEquals("feed:false", next())
        assertEquals("feed:true", nextMatching("feed:true"))
        assertEquals("event:Killed(key=a/b)", next())
        assertEquals(2, count("/api/events"))
        onMain { client.stop() }
        Thread.sleep(300)
        assertNull(seen.poll())  // closing it ourselves is not a loss
    }

    @Test fun aServerThatIsGoneIsReportedAfterAFewTries() {
        server.start()
        val base = server.url("/").toString().trimEnd('/')
        server.shutdown()
        val fast = Backoff(first = 10, max = 20)
        val losses = LinkedBlockingQueue<Int>()
        val feed = Feed(http, scope, { Request.Builder().url("$base/api/events").build() }, Wire::event, object : Feed.Listener<EventMsg> {
            override fun onMessage(message: EventMsg) = true
            override fun onLost(failures: Int) { losses += failures }
        }, fast)
        onMain { feed.start() }
        assertEquals(listOf(1, 2, 3), List(3) { losses.poll(5, TimeUnit.SECONDS) })
        onMain { feed.stop() }
        Thread.sleep(100)
        losses.clear()
        Thread.sleep(200)
        assertTrue(losses.isEmpty())  // stopped: no more tries
    }

    @Test fun aChatIsParsedOffTheMainThreadAndDeliveredOnIt() {
        serve(mapOf(
            "/api/events" to listOf(socket()),
            "/api/chat" to listOf(socket(
                """{"t":"open","key":"pc/a b","session":"s1","title":null}""",
                """{"t":"items","reset":true,"items":[{"id":"u1","ts":5,"kind":"user","text":"hi","images":[]}]}""",
                """{"t":"patch","id":"t1","output":"ok","error":false,"images":[]}""",
            )),
        ))
        val client = client()
        onMain {
            client.start()
            client.open("pc/a b")
        }
        assertEquals("chat:pc/a b:Open(key=pc/a b, session=s1, title=null):true", next())
        assertTrue(next().startsWith("chat:pc/a b:Items(reset=true, items=[ChatItem(id=u1"))
        assertEquals("chat:pc/a b:Patch(id=t1, output=ok, error=false, images=[]):true", next())
        assertEquals(listOf(false, false, false), prepared.toList())
        val request = requests.first { it.requestUrl!!.encodedPath == "/api/chat" }
        assertEquals("pc/a b", request.requestUrl!!.queryParameter("key"))
        assertEquals("http://${server.hostName}:${server.port}", request.getHeader("Origin"))
        onMain { client.stop() }
    }

    @Test fun noChatForGoodIsNotAskedAgainButAnUnreachableHostIs() {
        serve(mapOf(
            "/api/events" to listOf(socket()),
            "/api/chat" to listOf(socket("""{"t":"none","reason":"no Claude in this session"}""", close = 1000)),
        ))
        val client = client()
        onMain {
            client.start()
            client.open("pc/a")
        }
        assertEquals("chat:pc/a:None(reason=no Claude in this session):true", nextMatching("chat:"))
        Thread.sleep(1_500)
        assertEquals(1, count("/api/chat"))
        assertFalse(ChatClient.retryable("session ended"))
        assertTrue(ChatClient.retryable("host unreachable"))
        onMain { client.stop() }
    }

    @Test fun anUnreachableHostIsAskedAgain() {
        serve(mapOf(
            "/api/events" to listOf(socket()),
            "/api/chat" to listOf(socket("""{"t":"none","reason":"host unreachable"}""", close = 1000), socket("""{"t":"open","key":"pc/a","session":"s","title":"T"}""")),
        ))
        val client = client()
        onMain {
            client.start()
            client.open("pc/a")
        }
        assertEquals("chat:pc/a:None(reason=host unreachable):true", nextMatching("chat:"))
        assertEquals("chatlost:pc/a", nextMatching("chatlost"))
        assertEquals("chat:pc/a:Open(key=pc/a, session=s, title=T):true", nextMatching("chat:"))
        onMain { client.stop() }
    }

    @Test fun onlyTheHistoryResetsTheBackoff() {
        assertTrue(ChatClient.resetsBackoff(ChatMsg.Items(true, emptyList())))
        assertFalse(ChatClient.resetsBackoff(ChatMsg.Open("pc/a", "s", null)))
        assertFalse(ChatClient.resetsBackoff(ChatMsg.None("host unreachable")))
    }

    @Test fun sendKeyAndApprovePostJsonWithTheOrigin() {
        serve(mapOf(
            "/api/chat/send" to listOf(MockResponse().setBody("""{"ok":true}""")),
            "/api/chat/key" to listOf(MockResponse().setResponseCode(409).setBody("""{"ok":false,"error":"no Claude in this session"}""")),
            "/api/approve" to listOf(MockResponse().setResponseCode(502).setBody("<html>bad gateway</html>")),
        ))
        val client = client()
        assertEquals(Reply(true), onMain { client.send("pc/a", "line one\nline two") })
        val sent = requests.single()
        assertEquals("POST", sent.method)
        assertEquals("""{"key":"pc/a","text":"line one\nline two"}""", sent.body.readUtf8())
        assertEquals("http://${server.hostName}:${server.port}", sent.getHeader("Origin"))
        assertEquals("tmls_session=abc", sent.getHeader("Cookie"))
        assertTrue(sent.getHeader("Content-Type")!!.startsWith("application/json"))

        assertEquals(Reply(false, "no Claude in this session"), onMain { client.key("pc/a", "Escape") })
        assertEquals("""{"key":"pc/a","name":"Escape"}""", requests[1].body.readUtf8())

        val row = Row("pc/a", "pc", "desk", "a", "waiting", shown = listOf("Proceed?"), pane = "%1")
        assertEquals(Reply(false, "HTTP 502"), onMain { client.approve(row, true) })
        assertEquals("""{"host":"pc","name":"a","shown":["Proceed?"],"yes":true,"pane":"%1"}""", requests[2].body.readUtf8())
        assertNull(seen.poll())
    }

    @Test fun a401SendsTheUserToTheLogin() {
        serve(mapOf("/api/chat/send" to listOf(MockResponse().setResponseCode(401).setBody("""{"ok":false,"error":"login"}"""))))
        val reply = onMain { client().send("pc/a", "x") }
        assertTrue(reply.login)
        assertEquals("login", next())
    }

    @Test fun aServerThatDoesNotAnswerIsAnErrorNotAnException() {
        server.start()
        val client = client()
        server.shutdown()
        val reply = onMain { client.send("pc/a", "x") }
        assertFalse(reply.ok)
        assertTrue(reply.error!!.isNotBlank())
    }

    @Test fun anUploadSendsTheBytesAndReturnsThePath() {
        serve(mapOf("/api/chat/upload" to listOf(MockResponse().setBody("""{"ok":true,"path":"/home/u/.cache/tmls/images/p-1.jpg"}"""))))
        val bytes = ByteArray(200_000) { (it % 251).toByte() }
        val progress = CopyOnWriteArrayList<Float>()
        val reply = onMain { client().upload("pc/a", "my photo.jpg", bytes.size.toLong(), { ByteArrayInputStream(bytes) }) { progress += it } }
        assertEquals(Reply(true, path = "/home/u/.cache/tmls/images/p-1.jpg"), reply)
        val request = requests.single()
        assertEquals("pc/a", request.requestUrl!!.queryParameter("key"))
        assertEquals("my photo.jpg", request.requestUrl!!.queryParameter("name"))
        assertEquals(bytes.size.toLong(), request.bodySize)
        assertTrue(bytes.contentEquals(request.body.readByteArray()))
        assertEquals(1f, progress.last())
        assertEquals(progress.sorted(), progress.toList())
        assertEquals("http://${server.hostName}:${server.port}", request.getHeader("Origin"))
    }

    @Test fun anUploadOfUnknownSizeIsSentInChunks() {
        serve(mapOf("/api/chat/upload" to listOf(MockResponse().setBody("""{"ok":true,"path":"/p"}"""))))
        val bytes = ByteArray(70_000) { 7 }
        val reply = onMain { client().upload("pc/a", "f.bin", -1, { ByteArrayInputStream(bytes) }) {} }
        assertEquals("/p", reply.path)
        assertEquals(bytes.size.toLong(), requests.single().bodySize)
    }

    @Test fun tooLargeOrUnreadableUploadsAreErrors() {
        serve(mapOf("/api/chat/upload" to listOf(MockResponse().setBody("""{"ok":true,"path":"/p"}"""))))
        val big = onMain { client().upload("pc/a", "big.bin", Draft.MAX_UPLOAD + 1, { ByteArrayInputStream(ByteArray(1)) }) {} }
        assertFalse(big.ok)
        assertEquals(0, requests.size)
        val gone = onMain { client().upload("pc/a", "gone.bin", 10, { throw SecurityException("no access") }) {} }
        assertFalse(gone.ok)
        assertEquals("no access", gone.error)
    }
}
