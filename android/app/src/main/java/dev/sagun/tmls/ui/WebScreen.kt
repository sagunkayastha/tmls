package dev.sagun.tmls.ui

import android.annotation.SuppressLint
import android.content.ActivityNotFoundException
import android.content.Intent
import android.net.Uri
import android.view.View
import android.view.ViewGroup
import android.webkit.CookieManager
import android.webkit.RenderProcessGoneDetail
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.activity.compose.BackHandler
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.snapshotFlow
import kotlinx.coroutines.flow.distinctUntilChanged
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.viewinterop.AndroidView

/** The terminal's background; the WebView paints it before the page has. */
const val BG = 0xFF15191F.toInt()

/**
 * tmls-web, full screen. The site's own login page signs in; the WebView's cookie jar keeps the
 * session. No pull-to-refresh and no zoom: a swipe belongs to the terminal.
 * [onPainted] fires once the first page is on screen (the app shows a plain dark screen until then),
 * [onSignedIn] once a page other than /login has loaded, [onUpdateRequested] for the site's "⟳ App"
 * button, [onLeave] when Back has nothing left to close, [onCrashed] if the page's renderer died.
 */
@SuppressLint("SetJavaScriptEnabled")
@Composable
fun WebScreen(
    url: String,
    modifier: Modifier,
    keyboardOffsetDp: () -> Float,
    onPainted: () -> Unit,
    onSignedIn: () -> Unit,
    onUpdateRequested: () -> Unit,
    onLeave: () -> Unit,
    onCrashed: () -> Unit,
) {
    var webView by remember { mutableStateOf<WebView?>(null) }
    val baseUri = remember(url) { Uri.parse(url) }
    val signedIn by rememberUpdatedState(onSignedIn)

    // Back closes the site's modal / alert list, or on a session goes back to the list (tmlsBack in
    // app.js); only when nothing is left does the app go to the background, keeping the terminal.
    BackHandler {
        val web = webView ?: return@BackHandler onLeave()
        web.evaluateJavascript("typeof tmlsBack === 'function' && tmlsBack()") { handled ->
            if (handled != "true") onLeave()
        }
    }

    // While the keyboard slides, its reach above the page goes to the page every frame
    // (tmlsKeyboard in app.js moves the key bar with a transform: no relayout).
    LaunchedEffect(webView) {
        val web = webView ?: return@LaunchedEffect
        snapshotFlow { keyboardOffsetDp() }.distinctUntilChanged().collect { dp ->
            web.evaluateJavascript("typeof tmlsKeyboard === 'function' && tmlsKeyboard(${"%.1f".format(java.util.Locale.ROOT, dp)})", null)
        }
    }

    DisposableEffect(Unit) {
        onDispose {
            CookieManager.getInstance().flush()
            webView?.destroy()
        }
    }

    AndroidView(
        modifier = modifier,
        factory = { context ->
            var painted = false
            var signedInFired = false
            WebView(context).apply {
                // AndroidView's default WRAP_CONTENT makes Chromium size the page to its content, and
                // every vh/dvh unit is 0 (tmls-web's body is 100dvh: the page collapsed).
                layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT)
                setBackgroundColor(BG)
                overScrollMode = View.OVER_SCROLL_NEVER
                isVerticalScrollBarEnabled = false
                isHorizontalScrollBarEnabled = false
                settings.javaScriptEnabled = true       // xterm.js and the live row list
                settings.domStorageEnabled = true       // font size, last session
                settings.setSupportZoom(false)
                settings.builtInZoomControls = false
                settings.textZoom = 100                 // the terminal has its own A-/A+
                // The site shows the "⟳ App" button to this marker.
                settings.userAgentString = settings.userAgentString + " TmlsApp/1"
                CookieManager.getInstance().setAcceptCookie(true)
                webViewClient = object : WebViewClient() {
                    override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
                        if (sameOrigin(request.url, baseUri)) {
                            if (request.url.path != "/app/update") return false
                            onUpdateRequested()
                            return true
                        }
                        if (request.url.scheme in listOf("http", "https")) {
                            try {
                                context.startActivity(Intent(Intent.ACTION_VIEW, request.url))
                            } catch (e: ActivityNotFoundException) {
                                // no browser: nothing to open it with
                            }
                        }
                        return true
                    }

                    override fun onPageCommitVisible(view: WebView, url: String?) {
                        if (!painted) {
                            painted = true
                            onPainted()
                        }
                    }

                    override fun onPageFinished(view: WebView, finishedUrl: String?) {
                        CookieManager.getInstance().flush()
                        val path = Uri.parse(finishedUrl ?: "").path
                        if (!signedInFired && path != "/login") {
                            signedInFired = true
                            signedIn()
                        }
                    }

                    override fun onRenderProcessGone(view: WebView, detail: RenderProcessGoneDetail): Boolean {
                        // Android killed the page's renderer (memory): start over instead of crashing the app.
                        webView = null
                        view.destroy()
                        onCrashed()
                        return true
                    }
                }
                webView = this
                loadUrl(url)
            }
        },
    )
}

private fun sameOrigin(a: Uri, b: Uri): Boolean = a.scheme == b.scheme && a.host == b.host && a.port == b.port
