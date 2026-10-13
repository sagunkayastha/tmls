package dev.sagun.tmls.chat

import java.io.IOException
import java.io.InputStream
import java.util.concurrent.TimeUnit
import kotlin.coroutines.resume
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine
import okhttp3.Call
import okhttp3.Callback
import okhttp3.HttpUrl
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody
import okhttp3.RequestBody.Companion.toRequestBody
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import okio.BufferedSink

/**
 * One WebSocket that is kept open: when it drops it is opened again, later each time ([Backoff]).
 * [parse] runs on OkHttp's thread, so a long history never holds the screen; everything else,
 * [Listener] included, runs on [scope] (the main thread in the app), which is also the only place
 * [start] and [stop] may be called from.
 */
class Feed<T>(
    private val http: OkHttpClient,
    private val scope: CoroutineScope,
    private val request: () -> Request,
    private val parse: (String) -> T?,
    private val listener: Listener<T>,
    private val backoff: Backoff = Backoff(),
) {
    interface Listener<T> {
        fun onOpen() {}
        /** True when the message shows the feed works: the next reconnect starts from a short wait again. */
        fun onMessage(message: T): Boolean
        /** The socket is gone and will be reopened; [failures] in a row so far. */
        fun onLost(failures: Int) {}
        /** The server wants a login (close code 4401): not reopened. */
        fun onLogin() {}
        /** Refused for good (4403 wrong Origin, 4404 no such session): not reopened. */
        fun onRefused(code: Int) {}
    }

    private var running = false
    private var socket: WebSocket? = null
    private var retry: Job? = null
    private var failures = 0
    /** Counts sockets: what an old one still says after it was replaced or stopped is dropped. */
    private var gen = 0

    fun start() {
        if (running) return
        running = true
        backoff.reset()
        failures = 0
        connect()
    }

    fun stop() {
        running = false
        gen++
        retry?.cancel()
        retry = null
        socket?.close(1000, null)
        socket = null
    }

    private fun connect() {
        val mine = ++gen
        fun post(block: () -> Unit) {
            scope.launch { if (mine == gen) block() }
        }
        socket = http.newWebSocket(request(), object : WebSocketListener() {
            override fun onOpen(webSocket: WebSocket, response: Response) = post { listener.onOpen() }

            override fun onMessage(webSocket: WebSocket, text: String) {
                val message = parse(text) ?: return
                post {
                    if (listener.onMessage(message)) {
                        failures = 0
                        backoff.reset()
                    }
                }
            }

            override fun onClosing(webSocket: WebSocket, code: Int, reason: String) {
                webSocket.close(1000, null)
                post { ended(code) }
            }

            override fun onClosed(webSocket: WebSocket, code: Int, reason: String) = post { ended(code) }

            override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
                // A proxy may answer the upgrade itself instead of letting the server close with a code.
                val code = when (response?.code) {
                    401 -> LOGIN
                    403 -> REFUSED
                    else -> 0
                }
                post { ended(code) }
            }
        })
    }

    private fun ended(code: Int) {
        gen++  // onClosing and onClosed both report: only the first counts
        socket = null
        when (code) {
            LOGIN -> {
                running = false
                listener.onLogin()
            }
            REFUSED, NO_SESSION -> {
                running = false
                listener.onRefused(code)
            }
            else -> {
                failures++
                listener.onLost(failures)
                if (!running) return  // the listener stopped it
                val wait = backoff.next()
                retry = scope.launch {
                    delay(wait)
                    if (running && socket == null) connect()
                }
            }
        }
    }

    companion object {
        /** tmls-web's close codes (server.py, app.js). */
        const val LOGIN = 4401
        const val REFUSED = 4403
        const val NO_SESSION = 4404
    }
}

/**
 * The app's side of the chat contract for one server address: the session list (/api/events), the
 * chat of the session being looked at (/api/chat), and the POSTs. The login cookie comes from
 * [http]'s cookie jar (the WebView's, see WebkitCookieJar); the Origin is the site's own.
 * [start] in the foreground, [stop] in the background: no socket is held while nobody looks.
 */
class ChatClient(
    private val http: OkHttpClient,
    base: String,
    private val scope: CoroutineScope,
    private val sink: Sink,
) {
    /** All on [scope], except [prepare]. */
    interface Sink {
        fun onEvent(message: EventMsg)
        fun onFeed(up: Boolean)
        /** Several tries in a row found no server: the phone may have moved between home and away. */
        fun onUnreachable()
        /** On OkHttp's thread, before [onChat] gets the same message: the place for slow work (Markdown). */
        fun prepare(message: ChatMsg) {}
        fun onChat(key: String, message: ChatMsg)
        fun onChatLost(key: String)
        fun onLogin()
        fun onRefused(code: Int)
    }

    val urls = ChatUrls(base)
    private var started = false
    private var chatKey: String? = null
    private var chat: Feed<ChatMsg>? = null
    /** Uploads to a host reached over ssh take a while to be answered. */
    private val slow: OkHttpClient by lazy { http.newBuilder().readTimeout(UPLOAD_ANSWER_S, TimeUnit.SECONDS).build() }

    private fun ws(url: HttpUrl) = Request.Builder().url(url).header("Origin", urls.origin).build()

    private val events = Feed(http, scope, { ws(urls.events) }, Wire::event, object : Feed.Listener<EventMsg> {
        override fun onMessage(message: EventMsg): Boolean {
            sink.onFeed(true)
            sink.onEvent(message)
            return true
        }

        override fun onLost(failures: Int) {
            sink.onFeed(false)
            if (failures % UNREACHABLE_AFTER == 0) sink.onUnreachable()
        }

        override fun onLogin() = sink.onLogin()
        override fun onRefused(code: Int) = sink.onRefused(code)
    })

    fun start() {
        if (started) return
        started = true
        events.start()
        chat?.start()
    }

    fun stop() {
        started = false
        events.stop()
        chat?.stop()
    }

    /** The chat to follow, or null for none. Opening the same one again asks for its history again. */
    fun open(key: String?) {
        chat?.stop()
        chatKey = key
        chat = key?.let { k ->
            Feed(http, scope, { ws(urls.chat(k)) }, { text -> Wire.chat(text)?.also(sink::prepare) }, object : Feed.Listener<ChatMsg> {
                override fun onMessage(message: ChatMsg): Boolean {
                    sink.onChat(k, message)
                    if (message !is ChatMsg.None) return resetsBackoff(message)
                    // "none" is followed by a close. A host that did not answer is asked again, ever
                    // less often; the other reasons (session ended, no Claude in it) will not change
                    // by asking: the feed rests until it is opened again.
                    if (!retryable(message.reason)) chat?.stop()
                    return false
                }

                override fun onLost(failures: Int) = sink.onChatLost(k)
                override fun onLogin() = sink.onLogin()
                override fun onRefused(code: Int) = sink.onChat(k, ChatMsg.None(if (code == Feed.NO_SESSION) "no such session" else "refused by the server"))
            })
        }
        if (started) chat?.start()
    }

    suspend fun send(key: String, text: String, session: String? = null): Reply = post("api/chat/send", Wire.send(key, text, session))

    /** One of the contract's key names: Escape or Enter. */
    suspend fun key(key: String, name: String, session: String? = null): Reply = post("api/chat/key", Wire.key(key, name, session))

    suspend fun approve(row: Row, yes: Boolean, choice: Int? = null): Reply = post("api/approve", Wire.approve(row, yes, choice))

    /** The session was looked at: its "done" mark has been seen (as the web page does on a pick). */
    suspend fun seen(key: String): Reply = post("api/seen", Wire.seen(key))

    suspend fun logout(): Reply = post("logout", "", followRedirects = false)

    private suspend fun post(path: String, json: String, followRedirects: Boolean = true): Reply {
        val request = Request.Builder().url(urls.post(path)).header("Origin", urls.origin).post(json.toRequestBody(JSON)).build()
        val client = if (followRedirects) http else http.newBuilder().followRedirects(false).build()
        return call(client, request)
    }

    /**
     * [length] bytes (or -1 when unknown) from [open] to the session's host; the reply's path is
     * where it was stored. [onProgress] gets 0..1 on OkHttp's thread, only when the length is known.
     */
    suspend fun upload(key: String, name: String, length: Long, open: () -> InputStream, onProgress: (Float) -> Unit): Reply {
        if (length > Draft.MAX_UPLOAD) return Reply(false, "too large (32 MB at most)")
        val body = object : RequestBody() {
            override fun contentType() = BYTES
            override fun contentLength() = length

            override fun writeTo(sink: BufferedSink) {
                // On OkHttp's thread nothing but an IOException may escape: anything else would end the app.
                val stream = try {
                    open()
                } catch (e: RuntimeException) {
                    throw IOException(e.message ?: "could not read the file")
                }
                stream.use { input ->
                    val buf = ByteArray(64 * 1024)
                    var sent = 0L
                    while (true) {
                        val n = input.read(buf)
                        if (n < 0) break
                        sent += n
                        if (sent > Draft.MAX_UPLOAD) throw IOException("too large (32 MB at most)")
                        sink.write(buf, 0, n)
                        if (length > 0) onProgress((sent.toFloat() / length).coerceIn(0f, 1f))
                    }
                }
            }
        }
        val request = Request.Builder().url(urls.upload(key, name)).header("Origin", urls.origin).post(body).build()
        return call(slow, request)
    }

    /** Cancelling the caller cancels the request. A redirect that was not followed (logout) is success. */
    private suspend fun call(client: OkHttpClient, request: Request): Reply {
        val reply = suspendCancellableCoroutine { cont ->
            val call = client.newCall(request)
            cont.invokeOnCancellation { call.cancel() }
            call.enqueue(object : Callback {
                override fun onFailure(call: Call, e: IOException) =
                    cont.resume(Reply(false, e.message?.takeIf { it.isNotBlank() } ?: "no answer from the server"))

                override fun onResponse(call: Call, response: Response) = response.use {
                    val text = try {
                        it.peekBody(MAX_ANSWER).string()
                    } catch (e: IOException) {
                        ""
                    }
                    cont.resume(if (it.isRedirect) Reply(true) else Wire.reply(it.code, text))
                }
            })
        }
        if (reply.login) sink.onLogin()
        return reply
    }

    companion object {
        private val JSON = "application/json".toMediaType()
        private val BYTES = "application/octet-stream".toMediaType()
        private const val MAX_ANSWER = 64L * 1024
        private const val UPLOAD_ANSWER_S = 120L
        /** This many failed tries of the session list in a row, and the addresses are asked again. */
        const val UNREACHABLE_AFTER = 3

        /** The chat shows it works once its history arrives; `open` alone (an empty chat, a broken reader) does not. */
        fun resetsBackoff(message: ChatMsg): Boolean = message is ChatMsg.Items

        /** A "none" that asking again can cure. */
        fun retryable(reason: String): Boolean = reason.contains("unreachable", ignoreCase = true)

        /** Timeouts and keep-alive for the feeds and POSTs; the caller adds the cookie jar. */
        fun http(builder: OkHttpClient.Builder = OkHttpClient.Builder()): OkHttpClient.Builder = builder
            .connectTimeout(10, TimeUnit.SECONDS)
            .readTimeout(30, TimeUnit.SECONDS)
            .writeTimeout(30, TimeUnit.SECONDS)
            // A dead socket (Wi-Fi to mobile) is noticed in seconds instead of never.
            .pingInterval(20, TimeUnit.SECONDS)
    }
}
