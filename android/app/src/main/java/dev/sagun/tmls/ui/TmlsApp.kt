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
import androidx.compose.foundation.layout.WindowInsets
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
                // safeDrawing includes the keyboard, animated frame by frame as it slides.
                Column(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing)) {
                    UpdateBanner(updater)
                    WebScreen(
                        web.url,
                        Modifier.fillMaxSize(),
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
