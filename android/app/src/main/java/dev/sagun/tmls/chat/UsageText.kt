package dev.sagun.tmls.chat

import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale

/** How full a usage window or a context is: drawn normal, amber from 75 %, red from 90 %. */
enum class Fill { NORMAL, HIGH, FULL }

/** The drawer's usage rows and the context labels in words. Times are epoch seconds. */
object UsageText {
    /** Older than this, the drawer says how old its numbers are. */
    const val STALE_AFTER = 10 * 60L

    fun fill(pct: Int): Fill = when {
        pct >= 90 -> Fill.FULL
        pct >= 75 -> Fill.HIGH
        else -> Fill.NORMAL
    }

    /** "resets in 2h 14m", "resets in 14m", "resets in 1d 3h"; "reset" once the time is past or unknown. */
    fun countdown(resetsAt: Long?, now: Long): String {
        if (resetsAt == null || resetsAt <= now) return "reset"
        val minutes = (resetsAt - now + 59) / 60  // never "in 0m" while it is still ahead
        val d = minutes / (24 * 60)
        val h = minutes / 60 % 24
        val m = minutes % 60
        return "resets in " + when {
            d > 0 -> "${d}d ${h}h"
            h > 0 -> "${h}h ${m}m"
            else -> "${m}m"
        }
    }

    /** "resets Fri 9:00" in [zone]'s local time (12-hour "Fri 9:00 PM" unless [h24]); "reset" once past or unknown. */
    fun resetsOn(resetsAt: Long?, now: Long, zone: ZoneId, h24: Boolean, locale: Locale = Locale.getDefault()): String {
        if (resetsAt == null || resetsAt <= now) return "reset"
        val pattern = if (h24) "EEE H:mm" else "EEE h:mm a"
        return "resets " + DateTimeFormatter.ofPattern(pattern, locale).format(Instant.ofEpochSecond(resetsAt).atZone(zone))
    }

    /** "as of 12 min ago" once [at] is older than [STALE_AFTER], else null. */
    fun asOf(at: Long, now: Long): String? {
        val age = now - at
        if (age < STALE_AFTER) return null
        return "as of " + when {
            age < 3600 -> "${age / 60} min ago"
            age < 2 * 86400 -> "${age / 3600} h ago"
            else -> "${age / 86400} days ago"
        }
    }

    /** A session's context, labelled so it is not taken for anything else: "ctx 14%". */
    fun context(pct: Int): String = "ctx $pct%"
}
