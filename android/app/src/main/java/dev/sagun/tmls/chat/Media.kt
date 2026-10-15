package dev.sagun.tmls.chat

/**
 * A file sent to the session's host and named in the draft by its [path] there. [keep]: the user
 * wants it stored for good (~/Documents/tmls/YYYY-MM/ on that host) before the message goes, and
 * Claude given that path instead; [kept]: that is done, [path] is the kept one.
 */
data class Attached(val path: String, val name: String, val keep: Boolean = false, val kept: Boolean = false)

/** The attachments of each session's draft, as the drafts themselves are kept per session. */
object Attachments {
    /** Those of [list] still named in [text]: one taken out of the message is no longer attached. */
    fun shown(list: List<Attached>?, text: String): List<Attached> = list.orEmpty().filter { text.contains(it.path) }

    fun toggle(list: List<Attached>, path: String): List<Attached> =
        list.map { if (it.path == path && !it.kept) it.copy(keep = !it.keep) else it }

    /** [old] was kept as [new]: the message names the kept file, and the list says so. */
    fun keptAs(list: List<Attached>, old: String, new: String): List<Attached> =
        list.map { if (it.path == old) it.copy(path = new, keep = true, kept = true) else it }
}

/** Upload paths in messages already sent, for their Keep button. */
object Uploads {
    // ~/.cache/tmls/images/NAME or /any/home/.cache/tmls/images/NAME, as the server hands them out.
    private val PATH = Regex("""(?:~|/[^\s'"`]*?)/\.cache/tmls/images/[\p{L}\p{N}_.\-]+""")
    private val STORED = Regex("""\d{8}-\d{6}-[0-9a-f]{8}-(.+)""")
    private val HOME = Regex("""^/(?:home|Users)/[^/]+/|^/root/""")

    /** Each upload path in [text] once, in order (a full stop after one is not part of it). */
    fun inText(text: String): List<String> =
        PATH.findAll(text).map { it.value.trimEnd('.') }.filter { !it.endsWith("/images/") }.distinct().toList()

    /** The name the user would know: the original one kept inside the stored one, else the stored one. */
    fun displayName(path: String): String {
        val file = path.substringAfterLast('/')
        return STORED.matchEntire(file)?.groupValues?.get(1) ?: file
    }

    /** Where a kept picture of the chat is remembered in [ChatViewModel.keptSent]. */
    fun blobKey(id: String): String = "blob:$id"

    /** The name a sent photo is kept under: when it was sent, local time ([n]: which of several). */
    fun photoName(ts: Long, n: Int? = null, zone: java.time.ZoneId = java.time.ZoneId.systemDefault()): String {
        val at = java.time.Instant.ofEpochSecond(ts).atZone(zone)
        return "photo-" + java.time.format.DateTimeFormatter.ofPattern("yyyyMMdd-HHmmss").format(at) + (n?.let { "-$it" } ?: "")
    }

    /** /home/me/Documents/tmls/x -> ~/Documents/tmls/x, for showing where something went. */
    fun short(path: String): String = HOME.replace(path, "~/")
}

/** What another app shared into tmls: text (a link, a note) and/or files. */
data class Share(val text: String?, val subject: String?, val files: List<Picked>) {
    val empty: Boolean get() = text.isNullOrBlank() && files.isEmpty()

    /** One line saying what came, for the picker's title. */
    fun summary(): String = summary(files.map { it.mime }, text)

    /**
     * What goes into the message box: the user's own [message] first, then the shared text (with
     * its subject when the text does not already hold it, as browsers share a page's title).
     */
    fun compose(message: String): String {
        val shared = text?.trim().orEmpty()
        val title = subject?.trim().orEmpty()
        val body = if (title.isNotEmpty() && shared.isNotEmpty() && !shared.contains(title)) "$title\n$shared" else shared
        return listOf(message.trim(), body).filter { it.isNotEmpty() }.joinToString("\n")
    }

    companion object {
        fun summary(mimes: List<String?>, text: String?): String {
            val images = mimes.count { it?.startsWith("image/") == true }
            val parts = buildList {
                when {
                    mimes.isEmpty() -> {}
                    images == mimes.size -> add(if (images == 1) "1 image" else "$images images")
                    else -> add(if (mimes.size == 1) "1 file" else "${mimes.size} files")
                }
                val t = text?.trim().orEmpty()
                if (t.isNotEmpty()) add(if (t.startsWith("http://") || t.startsWith("https://")) "a link" else "text")
            }
            return parts.joinToString(" and ").ifEmpty { "nothing" }
        }

        /** [piece] after what [draft] already holds, on a line of its own. */
        fun append(draft: String, piece: String): String = when {
            piece.isEmpty() -> draft
            draft.isBlank() -> piece
            else -> draft.trimEnd() + "\n" + piece
        }

        /**
         * Sessions a share can go to, best first: Claude sessions on hosts that answer; the ones
         * waiting for the user, then the one looked at last, then the busy ones, then the rest
         * in the drawer's order.
         */
        fun targets(groups: List<HostGroup>, selected: String?): List<Row> {
            val all = groups.flatMap { it.rows }.filter { it.claude && it.online }
            return all.withIndex().sortedWith(compareBy({ !it.value.waiting }, { it.value.key != selected }, { !it.value.working }, { it.index }))
                .map { it.value }
        }
    }
}
