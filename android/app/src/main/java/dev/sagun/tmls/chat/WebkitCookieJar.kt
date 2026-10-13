package dev.sagun.tmls.chat

import android.webkit.CookieManager
import okhttp3.Cookie
import okhttp3.CookieJar
import okhttp3.HttpUrl

/**
 * The WebView's cookie jar, for OkHttp: the site's login page signs in inside the WebView, and the
 * native client sends the very same session cookie (and a logout here signs the WebView out too).
 */
class WebkitCookieJar : CookieJar {
    override fun loadForRequest(url: HttpUrl): List<Cookie> {
        val header = try {
            CookieManager.getInstance().getCookie(url.toString())
        } catch (e: RuntimeException) { // no WebView on the device right now (it is being updated)
            null
        } ?: return emptyList()
        // "name=value; name2=value2": values only, which is all a request sends.
        return header.split(';').mapNotNull { Cookie.parse(url, it.trim()) }
    }

    override fun saveFromResponse(url: HttpUrl, cookies: List<Cookie>) {
        val manager = try {
            CookieManager.getInstance()
        } catch (e: RuntimeException) {
            return
        }
        cookies.forEach { manager.setCookie(url.toString(), it.toString()) }
        manager.flush()
    }

    companion object {
        /** tmls-web's session cookie (auth.py). */
        const val SESSION = "tmls_session"

        /** Whether the WebView holds a login for [base]: without one, the login page comes first. */
        fun hasSession(base: String): Boolean = try {
            CookieManager.getInstance().getCookie(base).orEmpty().split(';').any { it.trim().startsWith("$SESSION=") }
        } catch (e: RuntimeException) {
            false
        }
    }
}
