package dev.sagun.tmls.ui

import android.content.Context
import android.content.ContextWrapper
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.displayCutout
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.systemBarsIgnoringVisibility
import androidx.compose.foundation.layout.union
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DrawerValue
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ModalNavigationDrawer
import androidx.compose.material3.Text
import androidx.compose.material3.rememberDrawerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.compose.LifecycleStartEffect
import dev.sagun.tmls.chat.ChatViewModel
import dev.sagun.tmls.chat.Row as SessionRow
import dev.sagun.tmls.update.Updater
import kotlinx.coroutines.launch

private tailrec fun Context.activity(): ComponentActivity? = when (this) {
    is ComponentActivity -> this
    is ContextWrapper -> baseContext.activity()
    else -> null
}

/**
 * The app once a server answers at [url]: the chat of one session, every session in a drawer, and
 * the site's own page in a WebView for what the chat is not — the login, sessions without Claude,
 * "Show terminal" and the New session form.
 * [onLeave] when Back has nothing left to close, [onChangeServer] for the Setup screen,
 * [onUnreachable] when the address stopped answering (the caller asks home and away again).
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun MainScreen(url: String, updater: Updater, onLeave: () -> Unit, onChangeServer: () -> Unit, onUnreachable: () -> Unit) {
    val activity = LocalContext.current.activity() ?: return
    val vm = remember(activity) { ViewModelProvider(activity, ChatViewModel.Factory(activity.application))[ChatViewModel::class.java] }
    val unreachable by rememberUpdatedState(onUnreachable)
    DisposableEffect(vm) {
        vm.onUnreachable = { unreachable() }
        onDispose { vm.onUnreachable = null }
    }
    // Sockets only in the foreground; another address (home/away) starts over on that one.
    LifecycleStartEffect(url) {
        vm.connect(url)
        vm.foreground(true)
        onStopOrDispose { vm.foreground(false) }
    }
    // As the page did once it had loaded past the login: ask for an update, quietly.
    var asked by remember(url) { mutableStateOf(false) }
    LaunchedEffect(vm.feedUp) {
        if (vm.feedUp && !asked) {
            asked = true
            updater.checkNow()
        }
    }
    val keyboard = rememberKeyboard()

    // The site's own pages, full screen: its login (the cookie it sets is the native client's too),
    // or all of it when the native client cannot be used on this device.
    if (vm.base != url || vm.needLogin || vm.webOnly) {
        if (vm.base != url) return  // the first frame, before the effect above has run
        Column(
            Modifier.fillMaxSize()
                .windowInsetsPadding(WindowInsets.systemBarsIgnoringVisibility.union(WindowInsets.displayCutout))
                .padding(bottom = keyboard.settledDp),
        ) {
            UpdateBanner(updater)
            key(vm.webGen) {
                WebScreen(
                    url,
                    Modifier.fillMaxSize(),
                    keyboardOffsetDp = { keyboard.slidingDp },
                    onPainted = {},
                    onSignedIn = {
                        vm.signedIn()
                        updater.checkNow()
                    },
                    onUpdateRequested = updater::checkManual,
                    onLeave = onLeave,
                    onCrashed = vm::webCrashed,
                )
            }
        }
        return
    }

    val drawer = rememberDrawerState(DrawerValue.Closed)
    val scope = rememberCoroutineScope()
    val softKeyboard = LocalSoftwareKeyboardController.current
    val row = vm.row
    val creating = vm.creating
    // The terminal: for the New session form, for a session without Claude, or because it was asked for.
    val terminal = creating != null || (row != null && (!row.claude || vm.terminal))

    // Nothing to show yet (first run, or the last session is gone): the list of sessions.
    LaunchedEffect(vm.listed) {
        if (vm.listed && vm.row == null) drawer.open()
    }
    LaunchedEffect(drawer.isOpen) {
        if (drawer.isOpen) softKeyboard?.hide()
    }

    // Lowest of the Back handlers: nothing left to close.
    BackHandler(onBack = onLeave)

    ModalNavigationDrawer(
        drawerState = drawer,
        // Over the terminal a sideways swipe belongs to the terminal.
        gesturesEnabled = drawer.isOpen || !terminal,
        drawerContent = {
            SessionDrawer(
                vm,
                onPick = { picked ->
                    vm.select(picked.key)
                    scope.launch { drawer.close() }
                },
                onNew = { host ->
                    vm.create(host)
                    scope.launch { drawer.close() }
                },
            )
        },
    ) {
        Column(Modifier.fillMaxSize().background(Tm.Bg)) {
            Column(Modifier.windowInsetsPadding(WindowInsets.statusBars.union(WindowInsets.displayCutout).only(WindowInsetsSides.Top + WindowInsetsSides.Horizontal))) {
                UpdateBanner(updater)
                TopBar(
                    title = when {
                        creating != null -> "New session"
                        row != null -> row.name
                        else -> "tmls"
                    },
                    subtitle = when {
                        creating != null -> creating.label
                        row != null -> if (row.online) row.label else "${row.label} · offline"
                        else -> ""
                    },
                    busy = creating == null && row?.working == true,
                    onMenu = { scope.launch { drawer.open() } },
                    items = buildList {
                        if (creating == null && row != null && row.claude) {
                            add((if (terminal) "Show chat" else "Show terminal") to { vm.showTerminal(!terminal) })
                        }
                        add("Reload" to vm::reload)
                        add("Check for updates" to updater::checkManual)
                        add("Change server" to onChangeServer)
                        add("Log out" to vm::logout)
                    },
                )
                if (vm.listed && !vm.feedUp) {
                    Text(
                        "live feed lost · reconnecting…", style = Tm.Small.copy(color = Tm.mark("waiting"), fontSize = 12.sp),
                        textAlign = TextAlign.Center, modifier = Modifier.fillMaxWidth().background(Tm.Raised).padding(vertical = 4.dp),
                    )
                }
            }
            Box(Modifier.weight(1f).fillMaxWidth()) {
                when {
                    terminal -> Box(
                        Modifier.fillMaxSize()
                            // As TmlsApp did for the whole page: the bars' stable size, the page
                            // shortened once per keyboard move (see Keyboard).
                            .windowInsetsPadding(
                                WindowInsets.systemBarsIgnoringVisibility.union(WindowInsets.displayCutout)
                                    .only(WindowInsetsSides.Horizontal + WindowInsetsSides.Bottom)
                            )
                            .padding(bottom = keyboard.settledDp),
                    ) {
                        key(vm.webGen) {
                            WebScreen(
                                url,
                                Modifier.fillMaxSize(),
                                keyboardOffsetDp = { keyboard.slidingDp },
                                onPainted = {},
                                onSignedIn = {},
                                onUpdateRequested = updater::checkManual,
                                onLeave = onLeave,
                                onCrashed = vm::webCrashed,
                                target = if (creating != null) WebTarget.New(creating.host, creating.label) else row?.let { WebTarget.Session(it.key) },
                                nativeBack = true,
                                backEnabled = !drawer.isOpen,
                                // Back from a terminal that was asked for: the chat again. From a session that
                                // has nothing but its terminal: out of the app, the session stays.
                                onBack = { if (creating != null || row?.claude == true) vm.showTerminal(false) else onLeave() },
                                onCurrent = vm::select,
                            )
                        }
                    }
                    row != null -> ChatScreen(
                        vm, row, onShowTerminal = { vm.showTerminal(true) },
                        modifier = Modifier.windowInsetsPadding(WindowInsets.displayCutout.only(WindowInsetsSides.Horizontal)),
                    )
                    !vm.listed -> Box(Modifier.fillMaxSize())
                    else -> Notice("Pick a session", "", "Sessions") { scope.launch { drawer.open() } }
                }
            }
        }
    }

    // Registered last, so asked first: an open drawer closes before anything under it hears of Back.
    BackHandler(enabled = drawer.isOpen) { scope.launch { drawer.close() } }
}

/** Triple bar, the session's name over its host, a spinner while Claude works, the menu. */
@Composable
private fun TopBar(title: String, subtitle: String, busy: Boolean, onMenu: () -> Unit, items: List<Pair<String, () -> Unit>>) {
    var menu by remember { mutableStateOf(false) }
    Row(Modifier.fillMaxWidth().height(60.dp).padding(horizontal = 6.dp), verticalAlignment = Alignment.CenterVertically) {
        IconButton(onClick = onMenu) { Icon(Glyphs.Menu, "Sessions", tint = Tm.Text) }
        // As wide on the left as the spinner and the menu on the right, so the name sits in the middle.
        Box(Modifier.size(width = 34.dp, height = 1.dp))
        Column(Modifier.weight(1f), horizontalAlignment = Alignment.CenterHorizontally) {
            Text(title, style = Tm.Body.copy(fontSize = 17.sp, lineHeight = 22.sp, fontWeight = androidx.compose.ui.text.font.FontWeight.SemiBold),
                maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (subtitle.isNotEmpty()) Text(subtitle, style = Tm.Small.copy(lineHeight = 18.sp), maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        Box(Modifier.size(34.dp), contentAlignment = Alignment.Center) {
            if (busy) CircularProgressIndicator(Modifier.size(20.dp), color = Tm.Accent, strokeWidth = 2.dp, trackColor = Tm.Line)
        }
        Box {
            IconButton(onClick = { menu = true }) { Icon(Glyphs.More, "Menu", tint = Tm.Text) }
            DropdownMenu(expanded = menu, onDismissRequest = { menu = false }, containerColor = Tm.Raised) {
                items.forEach { (label, action) ->
                    DropdownMenuItem(text = { Text(label, style = Tm.Body.copy(fontSize = 16.sp)) }, onClick = {
                        menu = false
                        action()
                    })
                }
            }
        }
    }
}
