package dev.sagun.tmls.chat

import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.temporal.ChronoUnit
import java.util.Locale

/** The items of one chat, in order. [apply] is the whole reducer: reset, append, patch. */
data class ChatLog(val items: List<ChatItem> = emptyList()) {
    fun apply(msg: ChatMsg): ChatLog = when (msg) {
        is ChatMsg.Items -> if (msg.reset) ChatLog(distinct(msg.items).takeLast(MAX)) else append(msg.items)
        is ChatMsg.Patch -> patch(msg)
        is ChatMsg.Open, is ChatMsg.None -> this
    }

    /** An id already in the list is replaced where it stands: ids are stable, so a re-send is an update. */
    private fun append(new: List<ChatItem>): ChatLog {
        if (new.isEmpty()) return this
        val fresh = distinct(new)
        val byId = fresh.associateBy { it.id }
        val known = items.mapTo(HashSet()) { it.id }
        val out = items.map { byId[it.id] ?: it } + fresh.filter { it.id !in known }
        return ChatLog(out.takeLast(MAX))
    }

    private fun patch(p: ChatMsg.Patch): ChatLog {
        val at = items.indexOfLast { it.id == p.id }
        if (at < 0) return this
        val old = items[at]
        val item = old.copy(output = p.output ?: old.output, error = p.error, images = p.images.ifEmpty { old.images })
        return ChatLog(items.toMutableList().also { it[at] = item })
    }

    companion object {
        /** The server sends at most 300 of history; a long day's appends stop growing here. */
        const val MAX = 1000

        /** The last of each id: the list's keys must be unique or the screen's list throws. */
        private fun distinct(items: List<ChatItem>): List<ChatItem> {
            val last = HashMap<String, Int>()
            items.forEachIndexed { i, item -> last[item.id] = i }
            return if (last.size == items.size) items else items.filterIndexed { i, item -> last[item.id] == i }
        }
    }
}

/** What the screen's list shows, one per entry; [key] is stable while the items it stands for are. */
sealed interface ChatRow {
    val key: String

    data class User(val item: ChatItem) : ChatRow { override val key get() = "i:" + item.id }
    data class Assistant(val item: ChatItem) : ChatRow { override val key get() = "i:" + item.id }
    data class Tool(val item: ChatItem) : ChatRow { override val key get() = "i:" + item.id }
    /** Consecutive tool calls folded into one row ("Ran 3 commands"); keyed by the first, so it stays put as it grows. */
    data class Tools(val items: List<ChatItem>) : ChatRow {
        override val key get() = "g:" + items.first().id
        val label: String get() = ChatRows.groupLabel(items)
    }
    data class Note(val item: ChatItem) : ChatRow { override val key get() = "i:" + item.id }
    /** A divider before an item that came long after the one before it. */
    data class Time(val before: String, val ts: Long) : ChatRow { override val key get() = "t:$before" }
}

object ChatRows {
    /** Items this far apart get a time divider between them. */
    const val GAP_S = 600L

    fun build(items: List<ChatItem>, gap: Long = GAP_S): List<ChatRow> {
        val out = ArrayList<ChatRow>(items.size + 8)
        val run = ArrayList<ChatItem>()
        fun flush() {
            if (run.size == 1) out.add(ChatRow.Tool(run[0])) else if (run.size > 1) out.add(ChatRow.Tools(run.toList()))
            run.clear()
        }
        var last = 0L
        for (item in items) {
            if (item.kind != Kind.TOOL && item.kind != Kind.NOTE && item.text.isBlank() && item.images.isEmpty()) continue
            if (last > 0 && item.ts > 0 && item.ts - last >= gap) {
                flush()
                out.add(ChatRow.Time(item.id, item.ts))
            }
            if (item.ts > 0) last = item.ts
            if (item.kind == Kind.TOOL) {
                run.add(item)
                continue
            }
            flush()
            out.add(
                when (item.kind) {
                    Kind.USER -> ChatRow.User(item)
                    Kind.ASSISTANT -> ChatRow.Assistant(item)
                    else -> ChatRow.Note(item)
                }
            )
        }
        flush()
        return out
    }

    /** The start of a long tool input or output, and whether anything was left out: the rest is one tap away. */
    fun head(text: String, lines: Int = 40, chars: Int = 3000): Pair<String, Boolean> {
        var end = 0
        var seen = 0
        while (end < text.length && end < chars) {
            val nl = text.indexOf('\n', end)
            if (nl < 0 || nl >= chars) {
                end = minOf(text.length, chars)
                break
            }
            seen++
            if (seen == lines) {
                end = nl
                break
            }
            end = nl + 1
        }
        return if (end >= text.length) text to false else text.substring(0, end).trimEnd('\n') to true
    }

    fun groupLabel(items: List<ChatItem>): String =
        if (items.all { it.tool == "Bash" }) "Ran ${items.size} commands" else "Used ${items.size} tools"

    private val CLOCK = DateTimeFormatter.ofPattern("HH:mm", Locale.ROOT)
    private val WEEKDAY = DateTimeFormatter.ofPattern("EEEE HH:mm", Locale.ENGLISH)
    private val DATE = DateTimeFormatter.ofPattern("d MMM HH:mm", Locale.ENGLISH)
    private val YEAR = DateTimeFormatter.ofPattern("d MMM yyyy HH:mm", Locale.ENGLISH)

    /** "00:02" today, "Monday 23:52" within the last week, "3 Oct 14:10" before that. */
    fun timeLabel(ts: Long, now: Long, zone: ZoneId): String {
        val at = Instant.ofEpochSecond(ts).atZone(zone)
        val today = Instant.ofEpochSecond(now).atZone(zone).toLocalDate()
        val days = ChronoUnit.DAYS.between(at.toLocalDate(), today)
        return when {
            days == 0L -> CLOCK.format(at)
            days in 1..6 -> WEEKDAY.format(at)
            at.year == today.year -> DATE.format(at)
            else -> YEAR.format(at)
        }
    }
}
