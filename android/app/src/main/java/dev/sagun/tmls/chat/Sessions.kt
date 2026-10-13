package dev.sagun.tmls.chat

/** The drawer's sessions of one host. */
data class HostGroup(val host: String, val label: String, val online: Boolean, val rows: List<Row>)

/** The rows of /api/events, kept and put in the drawer's order. */
object Sessions {
    /** The same marks as the web page (MARK in app.js). */
    fun glyph(mark: String): String = when (mark) {
        "running" -> "●"
        "waiting" -> "?"
        "done" -> "◆"
        "failed" -> "✕"
        "idle" -> "○"
        else -> ""
    }

    /** A rows message applied: a full one replaces everything (sessions that ended meanwhile go). */
    fun apply(rows: Map<String, Row>, msg: EventMsg.Rows): Map<String, Row> {
        val out = if (msg.full) LinkedHashMap() else LinkedHashMap(rows)
        msg.gone.forEach { out.remove(it) }
        msg.set.forEach { out[it.key] = it }
        return out
    }

    /**
     * Sessions by host, in the server's host order. A session waiting for the user comes first in its
     * host, and a host with one comes before the others; the rest by name. Other tmux servers
     * ("background" hosts, which the web page folds away) and hosts with no session are left out.
     */
    fun groups(rows: Collection<Row>, hosts: List<HostInfo>): List<HostGroup> {
        val info = hosts.associateBy { it.host }
        val order = LinkedHashMap<String, MutableList<Row>>()
        hosts.forEach { order[it.host] = mutableListOf() }
        rows.forEach { order.getOrPut(it.host) { mutableListOf() }.add(it) }
        return order.mapNotNull { (host, list) ->
            val background = info[host]?.background ?: list.any { it.background }
            if (list.isEmpty() || background) return@mapNotNull null
            HostGroup(
                host = host,
                label = info[host]?.label ?: list.first().label,
                online = info[host]?.online ?: list.any { it.online },
                rows = list.sortedWith(compareBy<Row> { !it.waiting }.thenBy(String.CASE_INSENSITIVE_ORDER) { it.name }),
            )
        }.sortedBy { group -> group.rows.none { it.waiting } }  // stable: the server's order within each half
    }

    /** Hosts a new session can be started on, as (host, label). */
    fun creatable(hosts: List<HostInfo>): List<HostInfo> = hosts.filter { it.online && !it.background }
}
