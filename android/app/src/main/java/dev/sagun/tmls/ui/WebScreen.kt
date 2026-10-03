package dev.sagun.tmls.ui

import android.annotation.SuppressLint
import android.content.ActivityNotFoundException
import android.content.Intent
import android.net.Uri
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout
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
import androidx.lifecycle.compose.LifecycleResumeEffect
import dev.sagun.tmls.BuildConfig
import android.Manifest
import android.content.ClipData
import android.content.ClipboardManager
import org.json.JSONObject
import android.content.pm.PackageManager
import android.os.Build
import android.util.Log
import android.webkit.ConsoleMessage
import android.webkit.ValueCallback
import android.webkit.WebChromeClient
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.platform.LocalContext
import androidx.core.content.ContextCompat
import androidx.core.content.FileProvider
import java.io.File
import androidx.webkit.JavaScriptReplyProxy
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import dev.sagun.tmls.Origins
import dev.sagun.tmls.Screenshots
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** The terminal's background; the WebView paints it before the page has. */
const val BG = 0xFF15191F.toInt()

/**
 * tmls-web, full screen. The site's own login page signs in; the WebView's cookie jar keeps the
 * session. No pull-to-refresh and no zoom: a swipe belongs to the terminal.
 * [onPainted] fires once the first page is on screen (the app shows a plain dark screen until then),
 * [onSignedIn] once a page other than /login has loaded, [onUpdateRequested] for the site's
 * "Check for updates", [onLeave] when Back has nothing left to close, [onCrashed] if the page's renderer died.
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

    // A page's <input type=file> (sketchpad's Image… > This device): Android's photo picker.
    var fileCallback by remember { mutableStateOf<ValueCallback<Array<Uri>>?>(null) }
    val pickImage = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri ->
        Log.i("tmls", "file chooser picked: ${uri != null}, callback waiting: ${fileCallback != null}")
        fileCallback?.onReceiveValue(uri?.let { arrayOf(it) })
        fileCallback = null
    }

    // A camera input (capture="environment", sketchpad's Camera…): the camera app takes the picture.
    var cameraUri by remember { mutableStateOf<Uri?>(null) }
    val takePicture = rememberLauncherForActivityResult(ActivityResultContracts.TakePicture()) { saved ->
        val uri = cameraUri
        fileCallback?.onReceiveValue(if (saved && uri != null) arrayOf(uri) else null)
        fileCallback = null
        cameraUri = null
    }

    // Sketch's "This phone's last screenshot": window.tmlsApp.postMessage("last-screenshot") from a
    // page of this server; the photos permission is asked the first time.
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var screenshotReply by remember { mutableStateOf<JavaScriptReplyProxy?>(null) }
    fun sendScreenshot(reply: JavaScriptReplyProxy) {
        scope.launch {
            val json = withContext(Dispatchers.IO) { Screenshots.latestAsJson(context) }
            reply.postMessage(json)
        }
    }
    val askPhotos = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        val reply = screenshotReply ?: return@rememberLauncherForActivityResult
        screenshotReply = null
        if (granted) sendScreenshot(reply)
        else reply.postMessage(Screenshots.error("tmls needs access to photos to read your screenshots."))
    }
    val photosPermission = if (Build.VERSION.SDK_INT >= 33) Manifest.permission.READ_MEDIA_IMAGES
        else Manifest.permission.READ_EXTERNAL_STORAGE

    // In the background or with the screen off, the page must know it's hidden: it lets go of its
    // tmux session then, so a laptop showing the same session gets its size back. The WebView
    // doesn't mark the page hidden by itself, so the app says so (tmlsVisible in app.js).
    // On pause, not stop: by stop Android has frozen the page and the message waits until it's back.
    LifecycleResumeEffect(webView) {
        webView?.evaluateJavascript("typeof tmlsVisible === 'function' && tmlsVisible(true)", null)
        onPauseOrDispose { webView?.evaluateJavascript("typeof tmlsVisible === 'function' && tmlsVisible(false)", null) }
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
            val web = TerminalWebView(context).apply {
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
                // The site shows "Check for updates" and this version at the end of its list to these markers.
                settings.userAgentString = settings.userAgentString + " TmlsApp/1 TmlsVersion/" + BuildConfig.VERSION_NAME
                CookieManager.getInstance().setAcceptCookie(true)
                webChromeClient = object : WebChromeClient() {
                    // The page's console in the phone's log (adb logcat -s tmls-page): the only
                    // window into the page in a release build.
                    override fun onConsoleMessage(message: ConsoleMessage): Boolean {
                        Log.i("tmls-page", "${message.messageLevel()} ${message.message()} (${message.sourceId()}:${message.lineNumber()})")
                        return true
                    }

                    override fun onShowFileChooser(view: WebView, callback: ValueCallback<Array<Uri>>,
                                                   params: FileChooserParams): Boolean {
                        fileCallback?.onReceiveValue(null)  // one picker at a time
                        fileCallback = callback
                        Log.i("tmls", "file chooser: ${params.acceptTypes.joinToString()} capture=${params.isCaptureEnabled}")
                        if (params.isCaptureEnabled) {
                            val photo = File(File(context.cacheDir, "camera").apply { mkdirs() }, "photo-${System.currentTimeMillis()}.jpg")
                            cameraUri = FileProvider.getUriForFile(context, "${context.packageName}.files", photo)
                            takePicture.launch(cameraUri!!)
                        } else {
                            pickImage.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly))
                        }
                        return true
                    }
                }
                if (WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)) {
                    // Offered to every frame, answered only for this server's pages (sketchpad's frame
                    // is the same host on another port, or a sibling name).
                    WebViewCompat.addWebMessageListener(this, "tmlsApp", setOf("*")) { _, message, origin, _, reply ->
                        if (!Origins.trusted(origin.toString(), url)) return@addWebMessageListener
                        val data = message.data ?: return@addWebMessageListener
                        val clipboard = context.getSystemService(ClipboardManager::class.java)
                        when {
                            data == "ime:terminal" -> { terminalInput(true); return@addWebMessageListener }
                            data == "ime:text" -> { terminalInput(false); return@addWebMessageListener }
                            data == "ime:none" -> { keyboardAway(); return@addWebMessageListener }
                            data == "ime:toggle" -> { toggleKeyboard(); return@addWebMessageListener }
                            // The key bar's Paste: the phone's clipboard, handed to the terminal.
                            data == "paste" -> {
                                val text = clipboard?.primaryClip?.takeIf { it.itemCount > 0 }
                                    ?.getItemAt(0)?.coerceToText(context)?.toString().orEmpty()
                                evaluateJavascript("typeof tmlsPaste === 'function' && tmlsPaste(${JSONObject.quote(text)})", null)
                                return@addWebMessageListener
                            }
                            // Copy's "Copy all" (Android shows its own "Copied").
                            data.startsWith("copy\n") -> {
                                clipboard?.setPrimaryClip(ClipData.newPlainText("tmls", data.removePrefix("copy\n")))
                                return@addWebMessageListener
                            }
                            data == "last-screenshot" -> {}
                            else -> return@addWebMessageListener
                        }
                        if (ContextCompat.checkSelfPermission(context, photosPermission) == PackageManager.PERMISSION_GRANTED) {
                            sendScreenshot(reply)
                        } else {
                            screenshotReply = reply
                            askPhotos.launch(photosPermission)
                        }
                    }
                }
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
            // The terminal's native input sits beside the page, invisible (see TerminalWebView).
            FrameLayout(context).apply {
                layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT)
                addView(web.keys, FrameLayout.LayoutParams(1, 1))
                addView(web, FrameLayout.LayoutParams(FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT))
            }
        },
    )
}

private fun sameOrigin(a: Uri, b: Uri): Boolean = a.scheme == b.scheme && a.host == b.host && a.port == b.port
