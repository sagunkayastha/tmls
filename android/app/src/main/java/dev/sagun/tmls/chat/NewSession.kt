package dev.sagun.tmls.chat

/** The New session form's rules, as plain values (the web page's form does the same in app.js). */
object NewSession {
    /** As create.default_name: the folder's last part, "home" for ~ (or nothing). */
    fun defaultName(folder: String): String {
        val f = folder.trim()
        if (f.trim('~', '/').isEmpty()) return "home"
        return f.trimEnd('/').substringAfterLast('/')
    }

    /** A start's name in the form. */
    fun label(start: Start): String = when (start.id) {
        "shell" -> "Shell"
        "claude" -> "Claude"
        "codex" -> "Codex"
        "opencode" -> "opencode"
        else -> start.id
    }

    /** It starts Claude: once Claude is up the new session opens as a chat. */
    fun startsClaude(start: Start): Boolean =
        start.id == "claude" || start.command?.trim()?.substringBefore(' ')?.substringAfterLast('/') == "claude"

    /** The choice a fresh form has: Claude, else the first. */
    fun defaultStart(starts: List<Start>): Start? = starts.firstOrNull { it.id == "claude" } ?: starts.firstOrNull()

    /** Why tmux can't take this name (as create.check_name), or null; checked here so the user hears at once. */
    fun nameError(name: String): String? {
        val n = name.trim()
        return when {
            n.isEmpty() -> "the session needs a name"
            ':' in n || '.' in n -> "names can't contain : or ."
            '#' in n -> "names can't contain #"
            n.startsWith("-") -> "names can't start with -"
            else -> null
        }
    }

    /** The folders to offer: home first (as ~), then the server's, each once. */
    fun choices(folders: Folders?): List<String> = (listOf("~") + folders?.folders.orEmpty()).distinct()
}
