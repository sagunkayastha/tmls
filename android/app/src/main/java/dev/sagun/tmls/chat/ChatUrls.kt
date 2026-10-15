package dev.sagun.tmls.chat

import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.TextFieldValue
import okhttp3.HttpUrl
import okhttp3.HttpUrl.Companion.toHttpUrl

/**
 * The addresses of one tmls-web ([base] is what Servers/Reach chose, like "http://192.168.0.5:8794")
 * and the Origin its middleware wants on WebSockets and POSTs: the site's own.
 */
class ChatUrls(val base: String) {
    private val root: HttpUrl = base.toHttpUrl()

    /** scheme://host[:port], as a browser on this site would send it: the server compares it to Host. */
    val origin: String = run {
        val host = if (':' in root.host) "[${root.host}]" else root.host
        val port = if (root.port == HttpUrl.defaultPort(root.scheme)) "" else ":${root.port}"
        "${root.scheme}://$host$port"
    }

    private fun api(path: String): HttpUrl.Builder = root.newBuilder().encodedPath("/").addPathSegments(path)

    /** OkHttp opens a WebSocket from an http(s) url. */
    val events: HttpUrl get() = api("api/events").build()
    fun chat(key: String): HttpUrl = api("api/chat").addQueryParameter("key", key).build()
    fun blob(key: String, id: String): HttpUrl =
        api("api/chat/blob").addQueryParameter("key", key).addQueryParameter("id", id).build()
    /** A picture file on the session's host, by its path there (absolute or ~/). */
    fun file(key: String, path: String): HttpUrl =
        api("api/chat/file").addQueryParameter("key", key).addQueryParameter("path", path).build()
    fun upload(key: String, name: String): HttpUrl =
        api("api/chat/upload").addQueryParameter("key", key).addQueryParameter("name", name).build()
    fun post(path: String): HttpUrl = api(path).build()
}

/** How long to wait before the next reconnect: doubling from [first] up to [max]. */
class Backoff(private val first: Long = 1_000, private val max: Long = 15_000) {
    private var next = first

    fun next(): Long = next.also { next = minOf(max, next * 2) }

    fun reset() {
        next = first
    }
}

/** The message being typed, as plain values. */
object Draft {
    /** The contract's upload limit. */
    const val MAX_UPLOAD = 32L * 1024 * 1024

    /**
     * [piece] (an uploaded file's path) put where the cursor is, replacing a selection, with a space
     * on each side where there is none. Returns the new text and where the cursor goes.
     */
    fun insert(text: String, start: Int, end: Int, piece: String): Pair<String, Int> {
        val a = minOf(start, end).coerceIn(0, text.length)
        val b = maxOf(start, end).coerceIn(0, text.length)
        val before = text.substring(0, a)
        val after = text.substring(b)
        val lead = if (before.isEmpty() || before.last().isWhitespace()) "" else " "
        val trail = if (after.isNotEmpty() && after.first().isWhitespace()) "" else " "
        val head = before + lead + piece + trail
        return head + after to head.length
    }

    /** A file name safe to put in a query and keep as a file's name: no folders, never empty. */
    fun fileName(name: String?, fallback: String): String {
        val clean = name.orEmpty().substringAfterLast('/').substringAfterLast('\\').trim().take(120)
        return clean.ifEmpty { fallback }
    }
}

/** The unsent message of each session, so picking another session and coming back keeps it. */
class Drafts {
    private val map = HashMap<String, TextFieldValue>()

    /** [current] is kept for [from] (if any); returns what [to] had, or an empty draft. */
    fun swap(from: String?, current: TextFieldValue, to: String): TextFieldValue {
        if (from != null) {
            if (current.text.isEmpty()) map.remove(from) else map[from] = current
        }
        return map[to] ?: TextFieldValue("")
    }

    /** [path] into [key]'s draft at its cursor, as it is put into the one being typed. */
    fun insert(key: String, path: String) {
        val cur = map[key] ?: TextFieldValue("")
        val (text, cursor) = Draft.insert(cur.text, cur.selection.start, cur.selection.end, path)
        map[key] = TextFieldValue(text, TextRange(cursor))
    }

    /** [old] (an upload path) became [new] (where it was kept) in [key]'s draft. */
    fun replace(key: String, old: String, new: String) {
        val cur = map[key] ?: return
        val text = cur.text.replace(old, new)
        if (text != cur.text) map[key] = TextFieldValue(text, TextRange(text.length))
    }

    /** [key] was sent: nothing left to keep. */
    fun clear(key: String) {
        map.remove(key)
    }
}
