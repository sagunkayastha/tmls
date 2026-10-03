package dev.sagun.tmls

import java.io.IOException
import java.util.concurrent.TimeUnit
import kotlin.coroutines.resume
import kotlinx.coroutines.async
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.selects.select
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withTimeoutOrNull
import okhttp3.Call
import okhttp3.Callback
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response

/** Which tmls-web answers: home on the LAN, else away. */
object Reach {
    private const val TIMEOUT_S = 3L
    /** A LAN answers in milliseconds; this long, home is preferred even when away answers first. */
    const val HOME_HEAD_START_MS = 500L

    /** Redirects off: /healthz answers 200 without a login; anything else is not tmls. */
    fun client(): OkHttpClient = OkHttpClient.Builder()
        .connectTimeout(TIMEOUT_S, TimeUnit.SECONDS)
        .readTimeout(TIMEOUT_S, TimeUnit.SECONDS)
        .callTimeout(TIMEOUT_S, TimeUnit.SECONDS)
        .followRedirects(false)
        .build()

    /**
     * A url from [urls] whose /healthz answers 200, or null. All are asked at once; the first gets
     * a short head start, after which whichever answers first wins. So off the LAN a home address
     * that hangs costs half a second, not the whole timeout.
     */
    suspend fun first(urls: List<String>, client: OkHttpClient): String? = coroutineScope {
        if (urls.isEmpty()) return@coroutineScope null
        val asks = urls.map { url -> async { if (healthy(client, url)) url else null } }
        val home = withTimeoutOrNull(HOME_HEAD_START_MS) { asks[0].await() }
        var answer: String? = home
        val open = asks.toMutableList()
        while (answer == null && open.isNotEmpty()) {
            val (ask, result) = select { open.forEach { a -> a.onAwait { a to it } } }
            open.remove(ask)
            answer = result
        }
        asks.forEach { it.cancel() }
        answer
    }

    /** Cancelling the caller cancels the request, so a hung address never holds anything up. */
    private suspend fun healthy(client: OkHttpClient, url: String): Boolean {
        val call = try {
            client.newCall(Request.Builder().url("$url/healthz").build())
        } catch (e: IllegalArgumentException) {
            return false
        }
        return suspendCancellableCoroutine { cont ->
            cont.invokeOnCancellation { call.cancel() }
            call.enqueue(object : Callback {
                override fun onFailure(call: Call, e: IOException) = cont.resume(false)
                override fun onResponse(call: Call, response: Response) = response.use { cont.resume(it.code == 200) }
            })
        }
    }
}
