package dev.sagun.tmls.chat

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.longOrNull
import kotlinx.serialization.json.put

/** One numbered choice of a permission prompt ("2. Yes, and always allow ..."). */
data class Choice(val n: Int, val label: String)

/** One session of one host, as /api/events lists it (rows.py), with the chat contract's three fields. */
data class Row(
    val key: String,
    val host: String,
    val label: String,
    val name: String,
    val mark: String,
    val line: String = "",
    /** The permission prompt's lines while the row waits on one, else null. */
    val shown: List<String>? = null,
    /** Why it waits ("permission prompt", "input needed"), as the server words it. */
    val waitingFor: String? = null,
    /** The prompt's numbered choices, when the server lists them. */
    val choices: List<Choice>? = null,
    /** Claude's own tmux pane ("%7"), for /api/approve. */
    val pane: String? = null,
    val online: Boolean = true,
    val background: Boolean = false,
    /** A Claude Code session runs in it: a chat exists. An older server never says so: terminal only. */
    val claude: Boolean = false,
    /** It opens as a chat: Claude or Codex runs in it. An older server says only [claude]. */
    val chat: Boolean = claude,
    /** Percent of Claude's context in use. */
    val context: Int? = null,
    val model: String? = null,
    /** The agent CLI in it: "claude", "codex", "opencode", another of herdr's ids; null for none (a shell). */
    val agent: String? = null,
) {
    val waiting: Boolean get() = mark == "waiting"
    val working: Boolean get() = mark == "running"
    /** Waiting on a permission prompt the app can answer. */
    val prompt: List<String>? get() = if (waiting) shown else null
    /** A permission dialog is up: a message typed now would answer it, so none is sent. */
    val asking: Boolean get() = shown != null
}

/** The button of a permission prompt that denies: the first choice after the first that starts with "No", else none. */
fun denyAt(choices: List<Choice>): Int? = choices.indexOfFirst { it.label.startsWith("No") }.takeIf { it > 0 }

data class HostInfo(val host: String, val label: String, val online: Boolean, val background: Boolean)

/** A choice of what a new session starts (/api/config `starts`): its id for /api/create and the command it runs. */
data class Start(val id: String, val command: String?)

/** A host's home and the folders worth offering for a new session (/api/folders), "~/…" under home. */
data class Folders(val home: String, val folders: List<String>)

data class Blob(val id: String, val mime: String)

enum class Kind { USER, ASSISTANT, TOOL, NOTE }

/** One item of a chat (CONTRACT.md, ITEM). Fields a kind does not carry keep their defaults. */
data class ChatItem(
    val id: String,
    val ts: Long,
    val kind: Kind,
    val text: String = "",
    val images: List<Blob> = emptyList(),
    val tool: String = "",
    val summary: String = "",
    val input: String = "",
    /** A tool's result; null until a patch brings it. */
    val output: String? = null,
    val error: Boolean = false,
)

/** One of Claude's usage windows: percent used, when it resets (null: already past), when the server's copy was written (epoch s). */
data class UsageWindow(val pct: Int, val resetsAt: Long?, val at: Long)

/** Claude's account-wide usage (/api/events `usage`); either window may be missing. [at]: the older of the two. */
data class Usage(val fiveHour: UsageWindow?, val sevenDay: UsageWindow?, val at: Long)

/** What /api/events sends. */
sealed interface EventMsg {
    data class Hosts(val hosts: List<HostInfo>) : EventMsg
    data class Rows(val full: Boolean, val set: List<Row>, val gone: List<String>) : EventMsg
    data class Renamed(val old: String, val new: String) : EventMsg
    data class Killed(val key: String) : EventMsg
    /** Claude's 5-hour and weekly usage; null when no host has it. */
    data class Usage(val usage: dev.sagun.tmls.chat.Usage?) : EventMsg
    /** version, alerts and whatever comes later: the feed is alive, nothing to do. */
    data object Other : EventMsg
}

/** What /api/chat sends. */
sealed interface ChatMsg {
    data class Open(val key: String, val session: String, val title: String?) : ChatMsg
    data class None(val reason: String) : ChatMsg
    data class Items(val reset: Boolean, val items: List<ChatItem>) : ChatMsg
    data class Patch(val id: String, val output: String?, val error: Boolean, val images: List<Blob>) : ChatMsg
}

/** The answer to a POST: {"ok":true,...} or {"ok":false,"error":...}; [login] for a 401. */
data class Reply(val ok: Boolean, val error: String? = null, val path: String? = null, val login: Boolean = false) {
    /** The answer's text, for the routes that answer with more than ok/error (folders, a suggested name); not part of equality. */
    var body: String = ""
        internal set
}

/** Contract messages to data classes and back. Anything not understood is null, never an exception. */
object Wire {
    private val json = Json { ignoreUnknownKeys = true }

    private fun obj(text: String): JsonObject? = try {
        json.parseToJsonElement(text) as? JsonObject
    } catch (e: IllegalArgumentException) { // includes SerializationException
        null
    }

    private fun JsonObject.str(name: String): String? = (this[name] as? JsonPrimitive)?.takeIf { it.isString }?.content
    private fun JsonObject.bool(name: String): Boolean? = (this[name] as? JsonPrimitive)?.takeIf { !it.isString }?.booleanOrNull
    private fun JsonObject.long(name: String): Long? = (this[name] as? JsonPrimitive)?.takeIf { !it.isString }?.longOrNull
    private fun JsonObject.list(name: String): List<JsonElement> = (this[name] as? JsonArray) ?: emptyList()
    private fun JsonObject.strings(name: String): List<String>? =
        (this[name] as? JsonArray)?.mapNotNull { (it as? JsonPrimitive)?.takeIf { p -> p.isString }?.content }

    fun event(text: String): EventMsg? {
        val o = obj(text) ?: return null
        return when (o.str("t")) {
            "hosts" -> EventMsg.Hosts(o.list("hosts").mapNotNull { host(it as? JsonObject ?: return@mapNotNull null) })
            "rows" -> EventMsg.Rows(
                full = o.bool("full") ?: false,
                set = o.list("set").mapNotNull { row(it as? JsonObject ?: return@mapNotNull null) },
                gone = o.strings("gone") ?: emptyList(),
            )
            "renamed" -> EventMsg.Renamed(o.str("old") ?: return null, o.str("new") ?: return null)
            "killed" -> EventMsg.Killed(o.str("key") ?: return null)
            "usage" -> EventMsg.Usage(usage(o["usage"] as? JsonObject))
            null -> null
            else -> EventMsg.Other
        }
    }

    private fun usage(o: JsonObject?): Usage? {
        if (o == null) return null
        val at = o.long("at") ?: return null
        fun window(name: String): UsageWindow? {
            val w = o[name] as? JsonObject ?: return null
            val pct = (w["pct"] as? JsonPrimitive)?.takeIf { !it.isString }?.content?.toDoubleOrNull() ?: return null
            return UsageWindow(pct.toInt().coerceIn(0, 100), w.long("resets_at"), w.long("at") ?: at)
        }
        val five = window("five_hour")
        val week = window("seven_day")
        return if (five == null && week == null) null else Usage(five, week, at)
    }

    private fun host(o: JsonObject): HostInfo? {
        val host = o.str("host") ?: return null
        return HostInfo(host, o.str("label") ?: host, o.bool("online") ?: true, o.bool("background") ?: false)
    }

    fun row(o: JsonObject): Row? {
        val key = o.str("key") ?: return null
        val host = o.str("host") ?: key.substringBefore('/')
        return Row(
            key = key,
            host = host,
            label = o.str("label") ?: host,
            name = o.str("name") ?: key.substringAfter('/'),
            mark = o.str("mark") ?: "idle",
            line = o.str("line") ?: "",
            shown = o.strings("shown"),
            waitingFor = o.str("waiting"),
            choices = (o["choices"] as? JsonArray)?.mapNotNull {
                val c = it as? JsonObject ?: return@mapNotNull null
                Choice((c.long("n") ?: return@mapNotNull null).toInt(), c.str("label") ?: return@mapNotNull null)
            },
            pane = o.str("pane"),
            online = o.bool("online") ?: true,
            background = o.bool("background") ?: false,
            claude = o.bool("claude") ?: false,
            chat = o.bool("chat") ?: o.bool("claude") ?: false,
            context = o.long("context")?.toInt(),
            model = o.str("model"),
            agent = o.str("agent"),
        )
    }

    /** /api/config's start choices; an older server lists only preset names: shell and claude, then those. */
    fun starts(text: String): List<Start>? {
        val o = obj(text)?.takeIf { "starts" in it || "presets" in it } ?: return null
        val starts = (o["starts"] as? JsonArray)?.mapNotNull {
            val s = it as? JsonObject ?: return@mapNotNull null
            Start(s.str("id") ?: return@mapNotNull null, s.str("command"))
        }
        return starts ?: (listOf(Start("shell", null), Start("claude", "claude")) + o.strings("presets").orEmpty().map { Start(it, null) })
    }

    fun folders(text: String): Folders? {
        val o = obj(text)?.takeIf { it.bool("ok") == true } ?: return null
        return Folders(o.str("home") ?: "", o.strings("folders").orEmpty())
    }

    /** /api/suggest-name's name, or null. */
    fun suggested(text: String): String? = obj(text)?.takeIf { it.bool("ok") == true }?.str("name")

    fun hostBody(host: String): String = buildJsonObject { put("host", host) }.toString()

    fun suggest(host: String, folder: String): String = buildJsonObject { put("host", host); put("folder", folder) }.toString()

    fun create(host: String, folder: String, name: String, start: String): String = buildJsonObject {
        put("host", host); put("folder", folder); put("name", name); put("start", start)
    }.toString()

    fun chat(text: String): ChatMsg? {
        val o = obj(text) ?: return null
        return when (o.str("t")) {
            "open" -> ChatMsg.Open(o.str("key") ?: "", o.str("session") ?: "", o.str("title"))
            "none" -> ChatMsg.None(o.str("reason") ?: "")
            "items" -> ChatMsg.Items(
                reset = o.bool("reset") ?: false,
                items = o.list("items").mapNotNull { item(it as? JsonObject ?: return@mapNotNull null) },
            )
            "patch" -> ChatMsg.Patch(o.str("id") ?: return null, o.str("output"), o.bool("error") ?: false, blobs(o))
            else -> null
        }
    }

    private fun blobs(o: JsonObject): List<Blob> = o.list("images").mapNotNull {
        val b = it as? JsonObject ?: return@mapNotNull null
        Blob(b.str("id") ?: return@mapNotNull null, b.str("mime") ?: "image/png")
    }

    /** A kind this app does not know becomes a grey line when it has text, and is dropped when not. */
    private fun item(o: JsonObject): ChatItem? {
        val id = o.str("id") ?: return null
        val ts = o.long("ts") ?: 0L
        val text = o.str("text") ?: ""
        return when (o.str("kind")) {
            "user" -> ChatItem(id, ts, Kind.USER, text = text, images = blobs(o))
            "assistant" -> ChatItem(id, ts, Kind.ASSISTANT, text = text)
            "tool" -> ChatItem(
                id, ts, Kind.TOOL, images = blobs(o), tool = o.str("tool") ?: "",
                summary = o.str("summary") ?: o.str("tool") ?: "Tool", input = o.str("input") ?: "",
                output = o.str("output"), error = o.bool("error") ?: false,
            )
            "note" -> ChatItem(id, ts, Kind.NOTE, text = text)
            else -> if (text.isBlank()) null else ChatItem(id, ts, Kind.NOTE, text = text)
        }
    }

    /** A POST's answer. A body that is not the contract's JSON (a proxy's error page) is "HTTP code". */
    fun reply(code: Int, body: String): Reply {
        if (code == 401) return Reply(ok = false, error = "login", login = true)
        val o = obj(body)
        val ok = code in 200..299 && o?.bool("ok") == true
        return Reply(ok, if (ok) null else o?.str("error") ?: "HTTP $code", o?.str("path")).also { it.body = body }
    }

    /** [session]: the id from the chat's `open`; the server refuses when Claude is on another conversation now. */
    fun send(key: String, text: String, session: String? = null): String =
        buildJsonObject { put("key", key); put("text", text); if (session != null) put("session", session) }.toString()

    fun key(key: String, name: String, session: String? = null): String =
        buildJsonObject { put("key", key); put("name", name); if (session != null) put("session", session) }.toString()

    fun seen(key: String): String = buildJsonObject { put("key", key) }.toString()

    /** /api/chat/keep's body: an upload [path] to store for good, under [name] (else its own). */
    fun keep(key: String, path: String, name: String? = null): String =
        buildJsonObject { put("key", key); put("path", path); if (name != null) put("name", name) }.toString()

    fun keepPicture(key: String, blob: String, name: String? = null): String =
        buildJsonObject { put("key", key); put("blob", blob); if (name != null) put("name", name) }.toString()

    /** /api/approve's body: the prompt as the row showed it, so the server refuses if it changed. */
    fun approve(row: Row, yes: Boolean, choice: Int? = null): String = buildJsonObject {
        put("host", row.host)
        put("name", row.name)
        put("shown", buildJsonArray { row.shown.orEmpty().forEach { add(JsonPrimitive(it)) } })
        put("yes", yes)
        if (choice != null) put("choice", choice)
        put("pane", row.pane?.let { JsonPrimitive(it) } ?: JsonNull)
    }.toString()

    /** [s] as a JavaScript string literal, for scripts handed to the terminal page. */
    fun quote(s: String): String = JsonPrimitive(s).toString().replace("\u2028", "\\u2028").replace("\u2029", "\\u2029")
}
