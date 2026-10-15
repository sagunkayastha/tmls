package dev.sagun.tmls.chat

import android.app.Application
import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.TextFieldValue
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import coil.ImageLoader
import java.io.File
import java.io.FileNotFoundException
import java.util.concurrent.ConcurrentHashMap
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import okhttp3.OkHttpClient

/** Where the chat of the selected session stands. */
sealed interface ChatPhase {
    /** No Claude session selected. */
    data object Idle : ChatPhase
    /** Asked, nothing back yet. */
    data object Loading : ChatPhase
    /** The server has no chat for it (no Claude, host unreachable, record not found). */
    data class None(val reason: String) : ChatPhase
    data object Open : ChatPhase
}

/** A file on its way to the session's host; [fraction] is null while the size is unknown. */
data class Upload(val name: String, val fraction: Float?)

/** One thing picked to attach; [temp] is the camera's own file, deleted after the upload; [mime] when known. */
data class Picked(val uri: Uri, val fallbackName: String, val temp: File? = null, val mime: String? = null)

/**
 * Everything the screens show: the sessions, the selected one, its chat, the message being typed.
 * It owns the [ChatClient] for the server address in use and replaces it when that address changes
 * (home/away, see TmlsApp). All of it lives on the main thread; Markdown is parsed off it.
 */
class ChatViewModel(app: Application) : AndroidViewModel(app), ChatClient.Sink, Voice.Sink {
    private val prefs = app.getSharedPreferences("chat", Context.MODE_PRIVATE)
    val http: OkHttpClient = ChatClient.http().cookieJar(WebkitCookieJar()).build()
    /** Pictures of a chat: fetched with the same client, so with the same login. */
    val images: ImageLoader = ImageLoader.Builder(app).okHttpClient(http).crossfade(true)
        // The server has a chat's pictures only while its feed is open: what was fetched once is kept.
        .respectCacheHeaders(false).build()

    private var client: ChatClient? = null
    private var foreground = false
    private var log = ChatLog()
    /** The session whose chat feed is open (null: none). */
    private var chatKey: String? = null
    /** Claude's session id from the last `open` of that chat: sent with messages, so a new conversation refuses them. */
    private var chatSession: String? = null
    private val parsed = ConcurrentHashMap<String, Pair<String, List<Block>>>()
    /** Logins the server asked for without the session list ever arriving in between. */
    private var bounces = 0

    var base by mutableStateOf<String?>(null)
        private set
    var rows by mutableStateOf<Map<String, Row>>(emptyMap())
        private set
    var hosts by mutableStateOf<List<HostInfo>>(emptyList())
        private set
    var groups by mutableStateOf<List<HostGroup>>(emptyList())
        private set
    /** The first full list of sessions has arrived. */
    var listed by mutableStateOf(false)
        private set
    /** The session list's feed is connected. */
    var feedUp by mutableStateOf(false)
        private set
    /** The session being looked at; remembered across restarts. */
    var selected by mutableStateOf<String?>(prefs.getString(CURRENT, null))
        private set
    var phase by mutableStateOf<ChatPhase>(ChatPhase.Idle)
        private set
    var title by mutableStateOf<String?>(null)
        private set
    var chatRows by mutableStateOf<List<ChatRow>>(emptyList())
        private set
    /** Goes up when the list should jump to its end whatever the user scrolled to (a message was sent). */
    var toBottom by mutableIntStateOf(0)
        private set
    /** Folded rows the user opened, by row key. */
    val unfolded = mutableStateMapOf<String, Boolean>()

    var draft by mutableStateOf(TextFieldValue(""))
    /** The other sessions' drafts; this ViewModel outlives configuration changes. */
    private val drafts = Drafts()
    var sending by mutableStateOf(false)
        private set
    var upload by mutableStateOf<Upload?>(null)
        private set
    var answering by mutableStateOf(false)
        private set
    /** Rows whose permission card is hidden: answered, and no rows message for them has come since. */
    var answered by mutableStateOf<Set<String>>(emptySet())
        private set
    var stopping by mutableStateOf(false)
        private set
    /** The last thing that went wrong, shown above the input until dismissed or something works. */
    var error by mutableStateOf<String?>(null)
    /** Something worth a line that did not go wrong (where a file was kept), until dismissed. */
    var notice by mutableStateOf<String?>(null)

    /** Uploaded files named in each session's draft, with their Keep marks (see [Attachments]). */
    var attached by mutableStateOf<Map<String, List<Attached>>>(emptyMap())
        private set
    /** Uploads in messages already sent that were kept: upload path -> kept path ("" while on its way). */
    val keptSent = mutableStateMapOf<String, String>()

    private val voice = Voice(app, this)
    /** Speech is being turned into text in the message box. */
    var listening by mutableStateOf(false)
        private set
    /** The session the words go to, and the draft around the cursor when listening began. */
    private var voiceKey: String? = null
    private var voiceBefore = ""
    private var voiceAfter = ""
    /** Stop was asked; the recognizer's last words are still to come. */
    private var voiceStopping = false

    /** The terminal instead of the chat, by the user's choice ("Show terminal"). */
    var terminal by mutableStateOf(false)
        private set
    /** The host a new session is being made on (the web page's form, in the terminal view). */
    var creating by mutableStateOf<HostInfo?>(null)
        private set
    /** A fresh terminal page for the same address ("Reload", or its renderer died). */
    var webGen by mutableIntStateOf(0)
        private set
    /** The site's login page is needed. */
    var needLogin by mutableStateOf(false)
        private set
    /** The native client cannot stay logged in on this device: the whole site in the WebView, as before. */
    var webOnly by mutableStateOf(false)
        private set

    /** Set by the screen: several tries found no server, so the addresses should be asked again. */
    var onUnreachable: (() -> Unit)? = null

    val row: Row? get() = selected?.let { rows[it] }

    /** Where a picture of the session's chat is fetched from. */
    fun blobUrl(key: String, id: String): String? = client?.urls?.blob(key, id)?.toString()

    // ---- the server address, foreground and background ----

    /** Use [url] (what Reach chose). The same one again changes nothing; another one starts over there. */
    fun connect(url: String) {
        if (url == base) return
        client?.stop()
        base = url
        feedUp = false
        listed = false
        bounces = 0
        webOnly = false
        needLogin = !WebkitCookieJar.hasSession(url)
        client = ChatClient(http, url, viewModelScope, this)
        chatKey = null
        resetChat()
        if (foreground && !needLogin) client?.start()
        syncChat()
    }

    /** Sockets only while the app is looked at. */
    fun foreground(on: Boolean) {
        foreground = on
        if (!on) stopVoice(cancel = true)  // never keep the microphone in the background
        if (on && !needLogin && !webOnly) client?.start() else if (!on) client?.stop()
    }

    /** The site's page loaded past its login: the cookie is there. */
    fun signedIn() {
        if (!needLogin) return
        needLogin = false
        if (foreground) client?.start()
    }

    /** New sockets and a new history, and a new terminal page when one shows. */
    fun reload() {
        webGen++
        error = null
        client?.stop()
        chatKey = null
        syncChat()
        if (foreground && !needLogin && !webOnly) client?.start()
    }

    fun logout() {
        val c = client ?: return
        viewModelScope.launch {
            c.logout()
            c.stop()
            needLogin = true
            images.memoryCache?.clear()  // pictures of the chats of the account that left
            withContext(Dispatchers.IO) { images.diskCache?.clear() }
        }
    }

    // ---- ChatClient.Sink ----

    override fun onFeed(up: Boolean) {
        if (up) bounces = 0
        feedUp = up
    }

    override fun onUnreachable() {
        onUnreachable?.invoke()
    }

    override fun onLogin() {
        client?.stop()
        feedUp = false
        bounces++
        // The WebView says it is signed in, the server tells the native client it is not, again:
        // its cookie does not reach here. Rather than bounce for ever, be the old app.
        if (bounces > MAX_BOUNCES) webOnly = true else needLogin = true
    }

    override fun onRefused(code: Int) {
        feedUp = false
        error = "The server refused the app's address (${client?.urls?.origin}). A proxy changing the Host header?"
    }

    override fun onEvent(message: EventMsg) {
        when (message) {
            is EventMsg.Hosts -> hosts = message.hosts
            is EventMsg.Rows -> {
                rows = Sessions.apply(rows, message)
                if (answered.isNotEmpty()) answered = answered - message.set.mapTo(HashSet()) { it.key }
                if (message.full) listed = true
            }
            is EventMsg.Renamed -> if (selected == message.old) remember(message.new)
            is EventMsg.Killed -> {}  // the rows message that follows drops it
            EventMsg.Other -> return
        }
        groups = Sessions.groups(rows.values, hosts)
        syncChat()
    }

    override fun prepare(message: ChatMsg) {
        if (message is ChatMsg.Items) message.items.forEach { if (it.kind == Kind.ASSISTANT) blocks(it) }
    }

    override fun onChat(key: String, message: ChatMsg) {
        if (key != chatKey) return
        when (message) {
            is ChatMsg.Open -> {
                chatSession = message.session.ifEmpty { null }
                title = message.title
                phase = ChatPhase.Open
            }
            is ChatMsg.None -> {
                phase = ChatPhase.None(message.reason.ifBlank { "no chat for this session" })
                show(ChatLog())
            }
            is ChatMsg.Items -> {
                phase = ChatPhase.Open
                show(log.apply(message))
                if (message.reset) {
                    val ids = log.items.mapTo(HashSet()) { it.id }
                    parsed.keys.retainAll(ids)
                }
            }
            is ChatMsg.Patch -> show(log.apply(message))
        }
    }

    override fun onChatLost(key: String) {
        // Kept on screen: the feed reopens by itself and sends the history again.
    }

    private fun show(next: ChatLog) {
        log = next
        chatRows = ChatRows.build(next.items)
    }

    /** A reply's Markdown as blocks: parsed once per item (off the main thread as it arrives), then kept. */
    fun blocks(item: ChatItem): List<Block> {
        parsed[item.id]?.let { (text, blocks) -> if (text == item.text) return blocks }
        val blocks = try {
            Markdown.parse(item.text)
        } catch (e: RuntimeException) { // a parser bug must not take the chat down: show the text as it is
            listOf(Block.Para(Rich(item.text)))
        }
        parsed[item.id] = item.text to blocks
        return blocks
    }

    // ---- sessions ----

    fun select(key: String) {
        creating = null
        terminal = false
        if (key == selected) return
        stopVoice(cancel = true)
        draft = drafts.swap(selected, draft, key)
        remember(key)
        error = null
        syncChat()
        val c = client ?: return
        viewModelScope.launch { c.seen(key) }
    }

    private fun remember(key: String) {
        selected = key
        prefs.edit().putString(CURRENT, key).apply()
    }

    fun showTerminal(on: Boolean) {
        creating = null
        terminal = on
    }

    fun create(host: HostInfo) {
        creating = host
    }

    /** The terminal page's renderer died: a new page. */
    fun webCrashed() {
        webGen++
    }

    /** The chat feed follows the selected session, when Claude runs in it. */
    private fun syncChat() {
        val want = row?.takeIf { it.claude }?.key
        if (want == chatKey) return
        chatKey = want
        resetChat()
        phase = if (want == null) ChatPhase.Idle else ChatPhase.Loading
        client?.open(want)
    }

    private fun resetChat() {
        chatSession = null
        title = null
        phase = ChatPhase.Idle
        unfolded.clear()
        parsed.clear()
        show(ChatLog())
    }

    // ---- the input bar ----

    fun send() {
        val c = client ?: return
        val key = selected ?: return
        val typed = draft.text
        if (typed.isBlank() || sending || row?.asking == true) return
        stopVoice(cancel = true)
        val session = chatSession
        sending = true
        viewModelScope.launch {
            // Files marked Keep are stored for good first, and Claude is given the kept path.
            var text = typed
            for (item in Attachments.shown(attached[key], typed).filter { it.keep && !it.kept }) {
                val kept = c.keep(key, item.path, item.name)
                val path = kept.path
                if (!kept.ok || path == null) {
                    sending = false
                    if (!kept.login) error = "Not sent: could not keep ${item.name}: ${kept.error ?: "no path came back"}"
                    return@launch
                }
                text = text.replace(item.path, path)
                attached = attached + (key to Attachments.keptAs(attached[key].orEmpty(), item.path, path))
                // the box (or the stored draft, after a switch) names the kept file too, whatever else changed
                if (selected == key) {
                    val now = draft.text.replace(item.path, path)
                    if (now != draft.text) draft = TextFieldValue(now, TextRange(now.length))
                } else {
                    drafts.replace(key, item.path, path)
                }
            }
            val reply = c.send(key, text, session)
            sending = false
            if (reply.ok) {
                error = null
                attached = attached - key
                if (selected != key) drafts.clear(key)  // sent after switching away: don't bring it back
                else if (draft.text == text) draft = TextFieldValue("")
                toBottom++
            } else if (!reply.login) {
                error = "Not sent: ${reply.error}"
            }
        }
    }

    /** Keep or don't keep an attachment of the message being typed. */
    fun toggleKeep(path: String) {
        val key = selected ?: return
        attached = attached + (key to Attachments.toggle(attached[key].orEmpty(), path))
    }

    /**
     * Keep a file from a message already sent: copied to ~/Documents/tmls/ on its host, and said where.
     * [path] is its upload path, or for a photo of the chat ([blob]) its [Uploads.blobKey].
     */
    fun keepSent(path: String, blob: String? = null, name: String? = null) {
        val c = client ?: return
        val key = chatKey ?: return
        if (keptSent[path] != null) return
        keptSent[path] = ""
        viewModelScope.launch {
            val reply = if (blob != null) c.keepPicture(key, blob, name) else c.keep(key, path)
            val kept = reply.path
            if (reply.ok && kept != null) {
                keptSent[path] = kept
                error = null
                notice = "Kept at ${Uploads.short(kept)}"
            } else {
                keptSent.remove(path)
                if (!reply.login) error = "Not kept: ${reply.error ?: "no path came back"}"
            }
        }
    }

    // ---- voice input ----

    /** Start turning speech into text at the cursor (the caller has the microphone permission). */
    fun startVoice() {
        val key = selected ?: return
        if (listening) return
        if (!voice.available()) {
            error = "Speech recognition isn't available on this phone."
            return
        }
        val text = draft.text
        val a = minOf(draft.selection.start, draft.selection.end).coerceIn(0, text.length)
        val b = maxOf(draft.selection.start, draft.selection.end).coerceIn(0, text.length)
        voiceKey = key
        voiceBefore = text.substring(0, a)
        voiceAfter = text.substring(b)
        error = null
        voiceStopping = false
        listening = voice.start()
        if (!listening) {
            voiceKey = null
            error = "Speech recognition isn't available on this phone."
        }
    }

    /** Stop listening: what was heard stays in the message box ([cancel]: at once, without the last words). */
    fun stopVoice(cancel: Boolean = false) {
        if (!listening) return
        // A second tap while the recognizer is still finishing: don't wait for it any longer.
        if (cancel || voiceStopping) {
            voice.cancel()
            listening = false
            voiceStopping = false
            voiceKey = null
        } else {
            voiceStopping = true
            voice.stop()
        }
    }

    override fun onWords(text: String, final: Boolean) {
        if (voiceKey == null || voiceKey != selected || text.isBlank()) return
        val (value, cursor) = Draft.insert(voiceBefore + voiceAfter, voiceBefore.length, voiceBefore.length, text.trim())
        draft = TextFieldValue(value, TextRange(cursor))
    }

    override fun onVoiceEnd(error: String?) {
        listening = false
        voiceStopping = false
        voiceKey = null
        if (error != null) this.error = error
    }

    // ---- shared into the app ----

    /**
     * What another app shared, sent to session [key]: that session is opened, the text goes into
     * its message box after what was there, and the files are uploaded like picked ones.
     */
    fun receiveShare(key: String, share: Share, message: String) {
        select(key)
        stopVoice(cancel = true)
        val piece = share.compose(message)
        if (piece.isNotEmpty()) {
            val text = Share.append(draft.text, piece)
            draft = TextFieldValue(text, TextRange(text.length))
        }
        if (share.files.isNotEmpty()) {
            if (upload != null) error = "Another upload is still running: share the files again when it is done."
            else attach(share.files)
        }
    }

    /** Escape to Claude's pane: stop what it is doing. */
    fun stop() {
        val c = client ?: return
        val key = selected ?: return
        if (stopping) return
        val session = chatSession
        stopping = true
        viewModelScope.launch {
            val reply = c.key(key, "Escape", session)
            stopping = false
            if (!reply.ok && !reply.login) error = "Not stopped: ${reply.error}"
        }
    }

    /** Allow or Deny on the permission prompt the row shows, or one of its numbered [choice]s. */
    fun answer(yes: Boolean, choice: Int? = null) {
        val c = client ?: return
        val r = row?.takeIf { it.prompt != null } ?: return
        if (answering) return
        answering = true
        viewModelScope.launch {
            val reply = c.approve(r, yes, choice)
            answering = false
            if (reply.ok) {
                error = null
                answered = answered + r.key  // a second tap must not answer the prompt that follows
            } else if (!reply.login) error = reply.error ?: "The answer did not get through."
        }
    }

    /**
     * A picked photo or file: sent to the session's host, its path there put into the message.
     * [temp] is the camera's own file in the app's cache, deleted once it is no longer needed.
     */
    fun attach(uri: Uri, fallbackName: String, temp: File? = null) = attach(listOf(Picked(uri, fallbackName, temp)))

    /** Several picked at once: sent one after the other, in the order picked; the first failure ends it. */
    fun attach(picked: List<Picked>) {
        val c = client ?: return
        val key = selected ?: return
        if (upload != null || picked.isEmpty()) return
        val resolver = getApplication<Application>().contentResolver
        upload = Upload(picked.first().fallbackName, null)
        viewModelScope.launch {
            for ((index, item) in picked.withIndex()) {
                val count = if (picked.size > 1) "${index + 1} of ${picked.size}: " else ""
                var fileName = Draft.fileName(null, item.fallbackName)
                val reply = try {
                    val (name, size) = withContext(Dispatchers.IO) {
                        var name: String? = null
                        var size = -1L
                        try {
                            resolver.query(item.uri, arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE), null, null, null)?.use { cursor ->
                                if (cursor.moveToFirst()) {
                                    name = cursor.getString(0)
                                    size = if (cursor.isNull(1)) -1L else cursor.getLong(1)
                                }
                            }
                        } catch (e: RuntimeException) {
                            // a provider that cannot be asked: the fallback name and an unknown size
                        }
                        if (size < 0 && item.temp != null) size = item.temp.length()
                        Draft.fileName(name, item.fallbackName) to size
                    }
                    fileName = name
                    upload = Upload(count + name, if (size > 0) 0f else null)
                    var shown = -1
                    c.upload(key, name, size, open = { resolver.openInputStream(item.uri) ?: throw FileNotFoundException(name) }) { fraction ->
                        val percent = (fraction * 100).toInt()
                        if (percent != shown) {  // once per percent, not once per block
                            shown = percent
                            viewModelScope.launch { if (upload != null) upload = Upload(count + name, fraction) }
                        }
                    }
                } catch (e: RuntimeException) { // the picker's grant is gone, the file was removed
                    Reply(false, e.message ?: "could not read the file")
                }
                withContext(Dispatchers.IO) { item.temp?.delete() }
                val path = reply.path
                if (reply.ok && path != null) {
                    error = null
                    if (voiceKey == key) stopVoice(cancel = true)  // its next words would rebuild the box without the path
                    attached = attached + (key to (attached[key].orEmpty() + Attached(path, fileName)))
                    if (selected == key) {
                        val (text, cursor) = Draft.insert(draft.text, draft.selection.start, draft.selection.end, path)
                        draft = TextFieldValue(text, TextRange(cursor))
                    } else {
                        drafts.insert(key, path)  // the session it was started in, not the one looked at now
                    }
                } else {
                    if (!reply.login) error = "Upload failed: ${reply.error ?: "no path came back"}"
                    break
                }
            }
            upload = null
        }
    }

    override fun onCleared() {
        voice.cancel()
        client?.stop()
        images.shutdown()
    }

    /** Made by hand, so nothing is looked up by reflection (R8 renames freely). */
    class Factory(private val app: Application) : ViewModelProvider.Factory {
        @Suppress("UNCHECKED_CAST")
        override fun <T : ViewModel> create(modelClass: Class<T>): T = ChatViewModel(app) as T
    }

    companion object {
        private const val CURRENT = "current"
        private const val MAX_BOUNCES = 2
    }
}
