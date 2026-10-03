package dev.sagun.tmls

import okhttp3.HttpUrl.Companion.toHttpUrlOrNull

/** Which pages may use the app's own channels (the phone's last screenshot). */
object Origins {
    private fun isAddress(host: String) = ':' in host || host.all { it.isDigit() || it == '.' }
    /**
     * [source] is the server [base] itself (any port: sketchpad is framed from the same host), or a
     * sibling name under the same parent domain (sketchpad.example.com beside tmls.example.com).
     */
    fun trusted(source: String, base: String): Boolean {
        val s = source.toHttpUrlOrNull()?.host ?: return false
        val b = base.toHttpUrlOrNull()?.host ?: return false
        if (s == b) return true
        if (isAddress(s) || isAddress(b)) return false  // addresses have no parent domain
        val sParent = s.substringAfter('.', "")
        val bParent = b.substringAfter('.', "")
        return bParent.contains('.') && sParent == bParent
    }
}
