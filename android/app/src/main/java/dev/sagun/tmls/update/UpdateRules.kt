package dev.sagun.tmls.update

import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json

/**
 * The pure half of the updater, ported from the archbox app (hub/android UpdateRules.java): what
 * /app/latest.json must say before the app offers an update, what counts as newer, and the
 * download's copy loop. Deliberately strict — anything not fully understood is no update at all,
 * because the quiet failure (no banner) is always safe. No android.*, so plain JUnit covers it.
 */
object UpdateRules {
    /** A corrupt manifest must not be able to start an unbounded download. */
    const val MAX_APK_BYTES = 200L * 1024 * 1024
    /** The updater reads at most this much of the response; a longer body is not our manifest. */
    const val MAX_BODY = 512

    @Serializable
    data class Manifest(val versionCode: Long, val versionName: String, val size: Long, val sha256: String)

    private val json = Json { ignoreUnknownKeys = true }
    private val SHA256 = Regex("[0-9a-f]{64}")

    /** The manifest, or null when any field is missing, malformed or out of range. */
    fun parse(body: String?): Manifest? {
        if (body.isNullOrEmpty() || body.length > MAX_BODY) return null
        val m = try {
            json.decodeFromString(Manifest.serializer(), body)
        } catch (e: IllegalArgumentException) { // includes SerializationException
            return null
        }
        if (m.versionCode <= 0 || m.size <= 0 || m.size > MAX_APK_BYTES) return null
        if (m.versionName.isEmpty() || m.versionName.length > 64 || !SHA256.matches(m.sha256)) return null
        return m
    }

    /** Only a strictly higher code is an update: re-publishing the same build must stay silent. */
    fun isNewer(installed: Long, available: Long): Boolean = available > installed

    fun digestMatches(expected: String, actual: String): Boolean = expected.equals(actual, ignoreCase = true)

    fun hex(digest: ByteArray): String = digest.joinToString("") { "%02x".format(it) }

    /**
     * Copies exactly [total] bytes, reporting whole-percent progress (capped at 99: 100 means
     * installed, which only the caller knows). A body shorter or longer than [total] is refused.
     */
    fun copy(input: InputStream, output: OutputStream, total: Long, onProgress: (Int) -> Unit): Long {
        val buf = ByteArray(64 * 1024)
        var have = 0L
        var shown = -1
        while (true) {
            val n = input.read(buf)
            if (n <= 0) break
            if (Thread.currentThread().isInterrupted) throw IOException("cancelled")
            have += n
            if (have > total) throw IOException("longer than the manifest says")
            output.write(buf, 0, n)
            val pct = minOf(99L, have * 100 / total).toInt()
            if (pct != shown) {
                shown = pct
                onProgress(pct)
            }
        }
        if (have != total) throw IOException("short download")
        return have
    }
}
