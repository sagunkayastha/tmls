package dev.sagun.tmls.ui

import android.os.SystemClock
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.Crossfade
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.displayCutout
import androidx.compose.foundation.layout.ime
import androidx.compose.foundation.layout.imeAnimationSource
import androidx.compose.foundation.layout.imeAnimationTarget
import androidx.compose.foundation.layout.navigationBarsIgnoringVisibility
import androidx.compose.foundation.layout.systemBarsIgnoringVisibility
import androidx.compose.foundation.layout.union
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.LifecycleStartEffect
import dev.sagun.tmls.Reach
import dev.sagun.tmls.Servers
import dev.sagun.tmls.update.Updater
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

private sealed interface Screen {
    data object Loading : Screen
    data class Setup(val message: String?) : Screen
    /** [gen] makes a fresh WebView for the same url (after its renderer died). */
    data class Web(val url: String, val gen: Int) : Screen
    data class Unreachable(val urls: List<String>) : Screen
}

private val Bg = Color(BG)

/** Back after this long away: the phone may have moved between home and away, so ask again. */
private const val RECHECK_AFTER_MS = 60_000L

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun TmlsApp(servers: Servers, updater: Updater, onServer: (String?) -> Unit, onLeave: () -> Unit) {
    var screen by remember { mutableStateOf<Screen>(Screen.Loading) }
    var attempt by remember { mutableIntStateOf(0) }
    var stoppedAt by remember { mutableLongStateOf(0L) }
    val client = remember { Reach.client() }
    val scope = rememberCoroutineScope()

    fun show(url: String?, urls: List<String>) {
        onServer(url)
        screen = if (url == null) Screen.Unreachable(urls) else Screen.Web(url, attempt)
    }

    LaunchedEffect(attempt) {
        val urls = servers.list()
        if (urls.isEmpty()) {
            screen = Screen.Setup(null)
            return@LaunchedEffect
        }
        if (screen !is Screen.Web) screen = Screen.Loading
        show(Reach.first(urls, client), urls)
    }

    LifecycleStartEffect(Unit) {
        val web = screen as? Screen.Web
        if (web != null && stoppedAt > 0 && SystemClock.elapsedRealtime() - stoppedAt > RECHECK_AFTER_MS) {
            scope.launch {
                // Only switch when another address answers: a page on the right server reconnects itself.
                val url = Reach.first(servers.list(), client)
                if (url != null && url != web.url && screen == web) show(url, servers.list())
            }
        }
        onStopOrDispose { stoppedAt = SystemClock.elapsedRealtime() }
    }

    MaterialTheme(colorScheme = darkColorScheme(background = Bg, surface = Bg)) {
        // The Surface gives text its light colour; the Box only stacks the site and the overlay.
        Surface(Modifier.fillMaxSize(), color = Bg) { Box(Modifier.fillMaxSize()) {
            val web = screen as? Screen.Web
            var painted by remember(web) { mutableStateOf(false) }
            if (web != null) key(web) {
                val keyboard = rememberKeyboard()
                Column(
                    Modifier.fillMaxSize()
                        // The bars' stable size: Samsung's navigation inset animates 0 -> 15 dp as the
                        // keyboard hides, and following it relaid the page 3-4 more times.
                        .windowInsetsPadding(WindowInsets.systemBarsIgnoringVisibility.union(WindowInsets.displayCutout))
                        .padding(bottom = keyboard.settledDp),
                ) {
                    UpdateBanner(updater)
                    WebScreen(
                        web.url,
                        Modifier.fillMaxSize(),
                        keyboardOffsetDp = { keyboard.slidingDp },
                        onPainted = { painted = true },
                        onSignedIn = updater::checkNow,
                        onUpdateRequested = updater::checkManual,
                        onLeave = onLeave,
                        onCrashed = { attempt++ },
                    )
                }
            }
            // Everything that is not the site sits on top and fades away once the site has painted.
            AnimatedVisibility(
                visible = web == null || !painted,
                enter = fadeIn(tween(150)),
                exit = fadeOut(tween(200)),
            ) {
                Box(Modifier.fillMaxSize().background(Bg).windowInsetsPadding(WindowInsets.safeDrawing)) {
                    Crossfade(targetState = if (web != null) Screen.Loading else screen, animationSpec = tween(200), label = "screen") { s ->
                        when (s) {
                            Screen.Loading, is Screen.Web -> Loading()
                            is Screen.Setup -> SetupScreen(servers.home, servers.away, s.message) { home, away ->
                                servers.save(home, away)
                                val urls = servers.list()
                                val url = Reach.first(urls, client)
                                if (url == null) "Neither address answered. On home Wi-Fi?" else null.also { show(url, urls) }
                            }
                            is Screen.Unreachable -> Unreachable(s.urls, onRetry = { attempt++ }, onChange = { screen = Screen.Setup(null) })
                        }
                    }
                }
            }
        } }
    }
}

/**
 * The keyboard, split so the page is resized once per keyboard move instead of every frame (each
 * WebView resize makes Chromium re-raster the whole page: 12-25 ms frames on the Samsung).
 * [settledDp]: how much the page is shortened — while opening it stays at the old size until the
 * keyboard is fully up; while closing it takes the new size at once. [slidingDp]: how far the
 * keyboard currently reaches above that, which the page's key bar follows with a transform.
 */
private class Keyboard(private val settled: () -> Float, private val sliding: () -> Float) {
    val settledDp: Dp get() = settled().dp
    val slidingDp: Float get() = sliding()
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun rememberKeyboard(): Keyboard {
    val density = LocalDensity.current
    val ime = WindowInsets.ime
    val source = WindowInsets.imeAnimationSource
    val target = WindowInsets.imeAnimationTarget
    val bars = WindowInsets.navigationBarsIgnoringVisibility
    fun px(insets: WindowInsets) = insets.getBottom(density)
    // Above the navigation bar, which the base padding already leaves room for.
    fun settledPx(): Int {
        val from = px(source)
        val to = px(target)
        val now = if (from != to) minOf(from, to) else px(ime)
        return (now - px(bars)).coerceAtLeast(0)
    }
    return remember(density) {
        Keyboard(
            settled = { with(density) { settledPx().toDp().value } },
            sliding = { with(density) { (px(ime) - px(bars) - settledPx()).coerceAtLeast(0).toDp().value } },
        )
    }
}

/** Nothing for the first moment (a LAN answers in milliseconds), then a spinner. */
@Composable
private fun Loading() {
    var slow by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) {
        delay(600)
        slow = true
    }
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        if (slow) CircularProgressIndicator()
    }
}

@Composable
private fun Unreachable(urls: List<String>, onRetry: () -> Unit, onChange: () -> Unit) {
    Column(
        modifier = Modifier.fillMaxSize().padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp, Alignment.CenterVertically),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text("Can't reach tmls", style = MaterialTheme.typography.headlineSmall)
        Text(urls.joinToString("\n"), style = MaterialTheme.typography.bodyMedium)
        Button(onClick = onRetry) { Text("Retry") }
        TextButton(onClick = onChange) { Text("Change server") }
    }
}
