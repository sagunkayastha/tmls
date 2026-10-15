package dev.sagun.tmls.chat

/** Picture files named in a message by their path on the session's host (the server's /api/chat/file). */
object ImagePaths {
    private const val EXT = "png|jpe?g|gif|webp"
    // Absolute or ~/, ending in a picture extension. Not a piece of something longer: not inside a
    // URL (://host/a.png), a word (x/a.png) or a longer name (a.png.bak, a.png/x).
    private val PATH = Regex("""(?<![\w/.~:\-])(?:~/|/)[^\s'"`<>()\[\]{}|*?]*?\.(?:$EXT)(?![\w\-/]|\.\w)""", RegexOption.IGNORE_CASE)
    private val NAME = Regex(""".*\.(?:$EXT)""", RegexOption.IGNORE_CASE)

    /** Each picture path in [text] once, in order. */
    fun find(text: String): List<String> = PATH.findAll(text).map { it.value }.filter { it.length > 2 }.distinct().toList()

    /** The pictures of [text] the app put there when it was sent: uploads and kept files, not paths typed. */
    fun attached(text: String): List<String> = find(text).filter { isKept(it) || Uploads.inText(it).isNotEmpty() }

    fun isImageName(path: String): Boolean = NAME.matches(path.substringAfterLast('/'))

    /** The file's own name, for a caption. */
    fun name(path: String): String = path.trimEnd('/').substringAfterLast('/')

    /** Kept for good already: in ~/Documents/tmls/ on its host (where Keep puts files). */
    fun isKept(path: String): Boolean = path.contains("/Documents/tmls/")
}

/** What the user's bubble shows of a sent message once its pictures are drawn above it. */
object SentText {
    // Claude Code's stand-in for a picture pasted into its prompt.
    private val TOKEN = Regex("""\[Image #\d+]""")
    // Claude Code's wrapper around a pasted multi-line message: <pasted_content id="51f6"> ... </pasted_content id="51f6">.
    private val PASTED = Regex("""</?pasted_content(\s+id="[^"]*")?\s*>""")

    /** [text] without the "[Image #N]" tokens, the pasted-message wrapper and without the picture [paths], tidied up. */
    fun clean(text: String, paths: List<String>): String {
        var out = PASTED.replace(TOKEN.replace(text, ""), "")
        for (path in paths.sortedByDescending { it.length }) out = out.replace(path, "")
        return out.lines().joinToString("\n") { it.replace(Regex("""[ \t]{2,}"""), " ").trimEnd() }
            .replace(Regex("""\n{3,}"""), "\n\n").trim()
    }
}

/** Picture bytes by what they are, not what they are called. */
object PictureBytes {
    fun sniff(data: ByteArray): String? {
        fun at(i: Int, vararg b: Int) = data.size >= i + b.size && b.indices.all { data[i + it] == b[it].toByte() }
        return when {
            at(0, 0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A) -> "image/png"
            at(0, 0xFF, 0xD8, 0xFF) -> "image/jpeg"
            at(0, 0x47, 0x49, 0x46, 0x38) -> "image/gif"
            at(0, 0x52, 0x49, 0x46, 0x46) && at(8, 0x57, 0x45, 0x42, 0x50) -> "image/webp"
            else -> null
        }
    }

    private val EXT = mapOf("image/png" to "png", "image/jpeg" to "jpg", "image/gif" to "gif", "image/webp" to "webp")

    /** A file name for the phone: [name] made safe, with the extension [mime] calls for. */
    fun fileName(name: String, mime: String?): String {
        val base = name.substringAfterLast('/').replace(Regex("""[^\p{L}\p{N}._-]+"""), "_").trim('.', '_', '-').ifEmpty { "picture" }
        val ext = EXT[mime] ?: return base
        val stem = base.substringBeforeLast('.', base)
        val have = base.substringAfterLast('.', "").lowercase()
        return if (have == ext || (ext == "jpg" && have == "jpeg")) base else "$stem.$ext"
    }
}
