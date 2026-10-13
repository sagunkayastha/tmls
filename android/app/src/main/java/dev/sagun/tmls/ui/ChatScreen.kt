package dev.sagun.tmls.ui

import android.content.ActivityNotFoundException
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.widget.Toast
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.gestures.scrollBy
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.systemBarsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.nestedscroll.NestedScrollConnection
import androidx.compose.ui.input.nestedscroll.NestedScrollSource
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import coil.compose.AsyncImage
import coil.request.ImageRequest
import coil.size.Size
import dev.sagun.tmls.chat.Blob
import dev.sagun.tmls.chat.ChatItem
import dev.sagun.tmls.chat.ChatPhase
import dev.sagun.tmls.chat.ChatRow
import dev.sagun.tmls.chat.ChatRows
import dev.sagun.tmls.chat.ChatViewModel
import dev.sagun.tmls.chat.Row as SessionRow
import java.time.ZoneId
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/** A picture to show full screen: where it comes from and the name it is cached under. */
private data class Picture(val url: String, val cacheKey: String)

/** What a row of the chat can ask of the screen around it. */
private class RowActions(
    val markdown: MarkdownActions,
    val showPicture: (Picture) -> Unit,
    /** Fold or unfold the row with this key. */
    val toggle: (String) -> Unit,
)

/**
 * One session as a chat: its items, newest at the bottom, and the input bar under them, above the
 * keyboard. [onShowTerminal] for a session whose chat the server cannot give.
 */
@Composable
fun ChatScreen(vm: ChatViewModel, row: SessionRow, onShowTerminal: () -> Unit, modifier: Modifier = Modifier) {
    val context = LocalContext.current
    var picture by remember { mutableStateOf<Picture?>(null) }
    var code by remember { mutableStateOf<Pair<String, String>?>(null) }
    val markdown = remember(context) {
        MarkdownActions(
            openLink = { url -> openLink(context, url) },
            copy = { text -> copy(context, text) },
            showCode = { label, text -> code = label to text },
        )
    }

    Column(modifier.fillMaxSize().navigationBarsPadding().imePadding()) {
        Box(Modifier.weight(1f).fillMaxWidth()) {
            val phase = vm.phase
            val rows = vm.chatRows
            when {
                phase is ChatPhase.None -> Notice("No chat for this session", phase.reason, "Show terminal", onShowTerminal)
                rows.isNotEmpty() -> ChatList(vm, row.key, rows, markdown) { picture = it }
                phase == ChatPhase.Open -> Notice("Nothing said yet", "Type a message below to start.", null) {}
                else -> Waiting()
            }
        }
        Composer(vm, row)
    }

    picture?.let { PictureViewer(vm, it) { picture = null } }
    code?.let { (label, text) -> CodeViewer(label, text, onCopy = { copy(context, text) }) { code = null } }
}

/** Nothing for the first moment, then a spinner: a chat on the LAN is there before it would show. */
@Composable
private fun Waiting() {
    var slow by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) {
        delay(500)
        slow = true
    }
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        if (slow) CircularProgressIndicator(Modifier.size(28.dp), color = Tm.Accent, strokeWidth = 2.5.dp)
    }
}

@Composable
internal fun Notice(title: String, text: String, action: String?, onAction: () -> Unit) {
    Column(
        Modifier.fillMaxSize().padding(32.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp, Alignment.CenterVertically),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(title, style = Tm.Body.copy(color = Tm.Dim))
        if (text.isNotEmpty()) Text(text, style = Tm.Small.copy(color = Tm.Faint), textAlign = TextAlign.Center)
        if (action != null) TextButton(onClick = onAction) { Text(action, color = Tm.Accent) }
    }
}

/** To the very end of the list: the last item's last line, whatever its height. */
private suspend fun LazyListState.toEnd() {
    val count = layoutInfo.totalItemsCount
    if (count == 0) return
    if (layoutInfo.visibleItemsInfo.lastOrNull()?.index != count - 1) scrollToItem(count - 1)
    scrollBy(1_000_000f)
}

@Composable
private fun ChatList(vm: ChatViewModel, sessionKey: String, rows: List<ChatRow>, markdown: MarkdownActions, onPicture: (Picture) -> Unit) {
    // A new list for a new session, starting at its end.
    val state = remember(sessionKey) { LazyListState(firstVisibleItemIndex = (rows.size - 1).coerceAtLeast(0)) }
    // Follow new items while the user is at the end; let go as soon as they scroll back.
    var stick by remember(sessionKey) { mutableStateOf(true) }
    val scope = rememberCoroutineScope()
    val zone = remember { ZoneId.systemDefault() }
    val actions = remember(markdown, sessionKey) {
        RowActions(markdown, onPicture) { key ->
            stick = false  // the row opens downwards: following the end would push it off the screen
            vm.unfolded[key] = vm.unfolded[key] != true
        }
    }
    // Only the user's finger gets here (a jump to the end is not a nested scroll): towards older items.
    val finger = remember(sessionKey) {
        object : NestedScrollConnection {
            override fun onPreScroll(available: Offset, source: NestedScrollSource): Offset {
                if (available.y > 0f) stick = false
                return Offset.Zero
            }
        }
    }
    LaunchedEffect(state) {
        snapshotFlow { state.isScrollInProgress }.collect { moving -> if (!moving && !state.canScrollForward) stick = true }
    }
    // New items, a taller last item, the keyboard coming up: whatever leaves something below the
    // screen while following is answered by going to the end again.
    LaunchedEffect(state) {
        snapshotFlow { Triple(state.layoutInfo.totalItemsCount, state.layoutInfo.viewportSize.height, state.canScrollForward) }
            .collect { (_, _, more) -> if (more && stick && !state.isScrollInProgress) state.toEnd() }
    }
    LaunchedEffect(vm.toBottom) {
        if (vm.toBottom > 0) {
            stick = true
            state.toEnd()
        }
    }
    val jump by remember(state) { derivedStateOf { !stick && state.canScrollForward } }
    val now = System.currentTimeMillis() / 1000

    Box(Modifier.fillMaxSize()) {
        LazyColumn(
            state = state,
            modifier = Modifier.fillMaxSize().nestedScroll(finger),
            contentPadding = PaddingValues(horizontal = 20.dp, vertical = 16.dp),
            verticalArrangement = Arrangement.spacedBy(18.dp),
        ) {
            items(rows, key = { it.key }, contentType = { it::class }) { row ->
                when (row) {
                    is ChatRow.User -> UserBubble(vm, sessionKey, row.item, actions)
                    is ChatRow.Assistant -> {
                        val blocks = remember(row.item.id, row.item.text) { vm.blocks(row.item) }
                        MarkdownView(blocks, actions.markdown, Modifier.fillMaxWidth().pointerInput(row.item.text) {
                            detectTapGestures(onLongPress = { actions.markdown.copy(row.item.text) })
                        })
                    }
                    is ChatRow.Tool -> ToolLine(vm, sessionKey, row.item, row.key, actions)
                    is ChatRow.Tools -> {
                        val open = vm.unfolded[row.key] == true
                        Column {
                            FoldLine(row.label, open, failed = row.items.any { it.error }) { actions.toggle(row.key) }
                            if (open) {
                                Column(Modifier.padding(start = 12.dp, top = 4.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                                    row.items.forEach { ToolLine(vm, sessionKey, it, "i:" + it.id, actions) }
                                }
                            } else {
                                row.items.forEach { Pictures(vm, sessionKey, it.images, actions) }
                            }
                        }
                    }
                    is ChatRow.Note -> Text(
                        row.item.text, style = Tm.Small, textAlign = TextAlign.Center, maxLines = 3,
                        overflow = TextOverflow.Ellipsis, modifier = Modifier.fillMaxWidth().padding(horizontal = 24.dp),
                    )
                    is ChatRow.Time -> TimeDivider(ChatRows.timeLabel(row.ts, now, zone))
                }
            }
        }
        if (jump) {
            Surface(
                color = Tm.Raised, shape = CircleShape, shadowElevation = 6.dp,
                border = androidx.compose.foundation.BorderStroke(1.dp, Tm.Line),
                modifier = Modifier.align(Alignment.BottomCenter).padding(bottom = 10.dp).size(44.dp).clip(CircleShape).clickable {
                    stick = true
                    scope.launch { state.toEnd() }
                },
            ) {
                Box(contentAlignment = Alignment.Center) { Icon(Glyphs.Down, "Jump to the newest", Modifier.size(20.dp), tint = Tm.Text) }
            }
        }
    }
}

/** What the user typed: a dark bubble on the right. A long press copies it. */
@Composable
private fun UserBubble(vm: ChatViewModel, sessionKey: String, item: ChatItem, actions: RowActions) {
    Box(Modifier.fillMaxWidth().padding(start = 44.dp), contentAlignment = Alignment.CenterEnd) {
        Surface(
            color = Tm.Sunken, shape = RoundedCornerShape(22.dp),
            modifier = Modifier.pointerInput(item.text) { detectTapGestures(onLongPress = { actions.markdown.copy(item.text) }) },
        ) {
            Column(Modifier.padding(horizontal = 18.dp, vertical = 12.dp)) {
                Pictures(vm, sessionKey, item.images, actions)
                if (item.text.isNotBlank()) Text(item.text, style = Tm.Body)
            }
        }
    }
}

/** One grey line with a chevron: the folded form of tool work. */
@Composable
private fun FoldLine(text: String, open: Boolean, failed: Boolean, onClick: () -> Unit) {
    Row(
        Modifier.clip(RoundedCornerShape(8.dp)).clickable(onClick = onClick).padding(vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text, style = Tm.Body.copy(color = if (failed) Tm.Fail else Tm.Dim), maxLines = 1,
            overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f, fill = false),
        )
        Icon(if (open) Glyphs.ChevronDown else Glyphs.ChevronRight, null, Modifier.padding(start = 8.dp).size(16.dp), tint = Tm.Faint)
    }
}

/** One tool call: its summary; unfolded, what went in and what came out, in a mono box. */
@Composable
private fun ToolLine(vm: ChatViewModel, sessionKey: String, item: ChatItem, key: String, actions: RowActions) {
    val open = vm.unfolded[key] == true
    Column {
        FoldLine(item.summary, open, item.error) { actions.toggle(key) }
        if (open) {
            Surface(color = Tm.Sunken, shape = RoundedCornerShape(12.dp), modifier = Modifier.fillMaxWidth().padding(top = 6.dp)) {
                Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    if (item.input.isNotEmpty()) Folded(item.input, item.summary, Tm.Text, actions)
                    val output = item.output
                    if (item.input.isNotEmpty() && (output == null || output.isNotEmpty())) HorizontalDivider(color = Tm.Line)
                    when {
                        output == null -> Text("running…", style = Tm.Mono.copy(color = Tm.Faint))
                        output.isNotEmpty() -> Folded(output, item.summary, if (item.error) Tm.Fail else Tm.Dim, actions)
                    }
                }
            }
        }
        Pictures(vm, sessionKey, item.images, actions)
    }
}

/** The start of a long text; the whole of it is one tap away, full screen. */
@Composable
private fun Folded(text: String, label: String, color: Color, actions: RowActions) {
    val (head, more) = remember(text) { ChatRows.head(text) }
    Column {
        Text(head, style = Tm.Mono.copy(fontSize = 13.sp, lineHeight = 18.sp, color = color))
        if (more) TextButton(onClick = { actions.markdown.showCode(label, text) }) { Text("Show all", color = Tm.Accent) }
    }
}

@Composable
private fun Pictures(vm: ChatViewModel, sessionKey: String, blobs: List<Blob>, actions: RowActions) {
    val context = LocalContext.current
    blobs.forEach { blob ->
        val picture = remember(sessionKey, blob.id, vm.base) {
            vm.blobUrl(sessionKey, blob.id)?.let { Picture(it, "$sessionKey/${blob.id}") }
        } ?: return@forEach
        val request = remember(picture) {
            ImageRequest.Builder(context).data(picture.url).memoryCacheKey(picture.cacheKey).diskCacheKey(picture.cacheKey).build()
        }
        AsyncImage(
            model = request,
            imageLoader = vm.images,
            contentDescription = "Picture. Tap for full screen.",
            contentScale = ContentScale.Fit,
            modifier = Modifier.padding(vertical = 6.dp).heightIn(min = 72.dp, max = 280.dp).clip(RoundedCornerShape(12.dp))
                .background(Tm.Raised).clickable { actions.showPicture(picture) },
        )
    }
}

/** A thin line with the time in its middle, where two items lie far apart in time. */
@Composable
private fun TimeDivider(label: String) {
    Row(Modifier.fillMaxWidth().padding(vertical = 6.dp), verticalAlignment = Alignment.CenterVertically) {
        HorizontalDivider(Modifier.weight(1f), color = Tm.Line)
        Text(label, style = Tm.Small.copy(color = Tm.Faint), modifier = Modifier.padding(horizontal = 14.dp))
        HorizontalDivider(Modifier.weight(1f), color = Tm.Line)
    }
}

/** A picture over the whole screen: pinch to zoom, drag to move, double tap to zoom in and back. */
@Composable
private fun PictureViewer(vm: ChatViewModel, picture: Picture, onClose: () -> Unit) {
    val context = LocalContext.current
    // Its own cache entry at full size: the one in the chat was decoded small.
    val request = remember(picture) {
        ImageRequest.Builder(context).data(picture.url).size(Size.ORIGINAL)
            .memoryCacheKey(picture.cacheKey + "/full").diskCacheKey(picture.cacheKey).build()
    }
    Dialog(onDismissRequest = onClose, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        var scale by remember { mutableFloatStateOf(1f) }
        var offset by remember { mutableStateOf(Offset.Zero) }
        Box(
            Modifier.fillMaxSize().background(Color.Black)
                .pointerInput(Unit) {
                    detectTransformGestures { _, pan, zoom, _ ->
                        scale = (scale * zoom).coerceIn(1f, 6f)
                        // Never further than the picture's own edge.
                        val reachX = size.width * (scale - 1f) / 2f
                        val reachY = size.height * (scale - 1f) / 2f
                        val moved = offset + pan
                        offset = Offset(moved.x.coerceIn(-reachX, reachX), moved.y.coerceIn(-reachY, reachY))
                    }
                }
                .pointerInput(Unit) {
                    detectTapGestures(onDoubleTap = {
                        if (scale > 1f) {
                            scale = 1f
                            offset = Offset.Zero
                        } else {
                            scale = 2.5f
                        }
                    })
                },
        ) {
            AsyncImage(
                model = request,
                imageLoader = vm.images,
                contentDescription = "Picture",
                contentScale = ContentScale.Fit,
                modifier = Modifier.fillMaxSize().graphicsLayer {
                    scaleX = scale
                    scaleY = scale
                    translationX = offset.x
                    translationY = offset.y
                },
            )
            IconButton(onClick = onClose, modifier = Modifier.align(Alignment.TopEnd).systemBarsPadding().padding(8.dp)) {
                Icon(Glyphs.Close, "Close", tint = Color.White)
            }
        }
    }
}

/** Code or a tool's whole output over the whole screen, scrolling both ways. */
@Composable
private fun CodeViewer(label: String, text: String, onCopy: () -> Unit, onClose: () -> Unit) {
    Dialog(onDismissRequest = onClose, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Column(Modifier.fillMaxSize().background(Tm.Sunken).systemBarsPadding()) {
            Row(Modifier.padding(start = 20.dp, end = 6.dp).height(52.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(label, style = Tm.Mono.copy(color = Tm.Dim), maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
                IconButton(onClick = onCopy) { Icon(Glyphs.Copy, "Copy", Modifier.size(20.dp), tint = Tm.Dim) }
                IconButton(onClick = onClose) { Icon(Glyphs.Close, "Close", Modifier.size(20.dp), tint = Tm.Dim) }
            }
            HorizontalDivider(color = Tm.Line)
            Box(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).horizontalScroll(rememberScrollState())) {
                Text(text, style = Tm.Mono, softWrap = false, modifier = Modifier.padding(20.dp))
            }
        }
    }
}

private fun openLink(context: Context, url: String) {
    try {
        context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)))
    } catch (e: ActivityNotFoundException) {
        // no app to open it with
    }
}

internal fun copy(context: Context, text: String) {
    val clipboard = context.getSystemService(ClipboardManager::class.java) ?: return
    try {
        clipboard.setPrimaryClip(ClipData.newPlainText("tmls", text))
    } catch (e: RuntimeException) { // more than a clipboard can take
        Toast.makeText(context, "Too long to copy", Toast.LENGTH_SHORT).show()
        return
    }
    // Android 13 and later show their own "Copied".
    if (Build.VERSION.SDK_INT < 33) Toast.makeText(context, "Copied", Toast.LENGTH_SHORT).show()
}
