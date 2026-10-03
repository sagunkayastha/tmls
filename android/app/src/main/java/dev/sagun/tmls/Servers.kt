package dev.sagun.tmls

import android.content.Context
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull

/** Turns what the user typed into a base URL like "http://192.168.0.5:8794", or null. */
object ServerUrl {
    // Only a scheme ("http://", "https:///") has no host once the trailing slash is stripped;
    // reject it before that stripping folds it into something that looks valid.
    private val SCHEME_ONLY = Regex("^https?:/*$", RegexOption.IGNORE_CASE)

    fun normalize(input: String): String? {
        val trimmed = input.trim()
        if (trimmed.isEmpty() || SCHEME_ONLY.matches(trimmed)) return null
        var s = trimmed.trimEnd('/')
        if (!s.startsWith("http://") && !s.startsWith("https://")) s = "http://$s"
        val url = s.toHttpUrlOrNull() ?: return null
        return if (url.host.isEmpty() || s.contains(' ')) null else s
    }
}

/**
 * The two addresses typed on the Setup screen: home (the LAN) and, optionally, away (the public
 * name). No address is built in: the repo is public.
 */
class Servers(context: Context) {
    private val prefs = context.getSharedPreferences("servers", Context.MODE_PRIVATE)

    val home: String? get() = prefs.getString(HOME, null)
    val away: String? get() = prefs.getString(AWAY, null)

    /** What to try, in order. */
    fun list(): List<String> = order(home, away)

    fun save(home: String, away: String?) {
        prefs.edit().putString(HOME, home).putString(AWAY, away).apply()
    }

    companion object {
        private const val HOME = "home"
        private const val AWAY = "away"

        fun order(home: String?, away: String?): List<String> = listOfNotNull(home, away).distinct()
    }
}
