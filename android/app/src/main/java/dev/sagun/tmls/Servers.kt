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
 * Home (the LAN) and, optionally, away (the public name). A build can carry defaults (make.sh reads
 * them from ~/.config/tmls-android/servers, never from the repo); addresses saved on the Setup
 * screen ("Change server") replace them.
 */
class Servers(context: Context) {
    private val prefs = context.getSharedPreferences("servers", Context.MODE_PRIVATE)

    private val saved get() = prefs.getString(HOME, null)

    val home: String? get() = saved ?: BuildConfig.DEFAULT_HOME.ifEmpty { null }
    val away: String? get() = if (saved != null) prefs.getString(AWAY, null) else BuildConfig.DEFAULT_AWAY.ifEmpty { null }

    /** What to try, in order. */
    fun list(): List<String> =
        effective(saved, prefs.getString(AWAY, null), BuildConfig.DEFAULT_HOME, BuildConfig.DEFAULT_AWAY)

    fun save(home: String, away: String?) {
        prefs.edit().putString(HOME, home).putString(AWAY, away).apply()
    }

    companion object {
        private const val HOME = "home"
        private const val AWAY = "away"

        fun order(home: String?, away: String?): List<String> = listOfNotNull(home, away).distinct()

        /** Saved addresses if any were saved, else the build's defaults ("" = none). */
        fun effective(savedHome: String?, savedAway: String?, defaultHome: String, defaultAway: String): List<String> =
            if (savedHome != null) order(savedHome, savedAway)
            else order(defaultHome.ifEmpty { null }, defaultAway.ifEmpty { null })
    }
}
