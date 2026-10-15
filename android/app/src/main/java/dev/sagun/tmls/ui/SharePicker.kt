package dev.sagun.tmls.ui

import android.content.ClipData
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.webkit.MimeTypeMap
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.IntentCompat
import dev.sagun.tmls.chat.ChatViewModel
import dev.sagun.tmls.chat.Picked
import dev.sagun.tmls.chat.Sessions
import dev.sagun.tmls.chat.Share

/** The most files taken from one share, as from one pick. */
private const val MAX_SHARED = 10

/** What was shared into the app and not yet sent to a session; set by MainActivity. */
object ShareInbox {
    var pending by mutableStateOf<Share?>(null)

    /** Android's share intent (one item or several) as a [Share]; null for anything else or nothing in it. */
    fun read(context: Context, intent: Intent?): Share? {
        if (intent == null) return null
        val uris = when (intent.action) {
            Intent.ACTION_SEND -> listOfNotNull(IntentCompat.getParcelableExtra(intent, Intent.EXTRA_STREAM, Uri::class.java))
            Intent.ACTION_SEND_MULTIPLE -> IntentCompat.getParcelableArrayListExtra(intent, Intent.EXTRA_STREAM, Uri::class.java).orEmpty()
            else -> return null
        }.ifEmpty { intent.clipData?.uris().orEmpty() }  // some apps put the files only there
        val text = intent.getCharSequenceExtra(Intent.EXTRA_TEXT)?.toString()
            ?: intent.getCharSequenceArrayListExtra(Intent.EXTRA_TEXT)?.joinToString("\n")
        val now = System.currentTimeMillis()
        // content:// only: a file:// path could name this app's own private files (its login)
        val files = uris.filter { it.scheme == "content" && it.authority?.startsWith(context.packageName) != true }.distinct().take(MAX_SHARED).mapIndexed { i, uri ->
            val mime = try { context.contentResolver.getType(uri) } catch (e: RuntimeException) { null } ?: intent.type?.takeIf { '*' !in it }
            val ext = mime?.let { MimeTypeMap.getSingleton().getExtensionFromMimeType(it) }?.let { ".$it" } ?: ""
            Picked(uri, "shared-$now-${i + 1}$ext", mime = mime)
        }
        val share = Share(text, intent.getStringExtra(Intent.EXTRA_SUBJECT), files)
        return if (share.empty) null else share
    }

    private fun ClipData.uris(): List<Uri> = (0 until itemCount).mapNotNull { getItemAt(it).uri }
}

/**
 * Where a share goes: a Claude session (those waiting on the user first), an optional message,
 * then Attach. The text goes into that session's message box, the files are uploaded like picked
 * ones; nothing is sent until the user presses send there.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SharePicker(vm: ChatViewModel, share: Share, onDone: () -> Unit) {
    val targets = Share.targets(vm.groups, vm.selected)
    var picked by rememberSaveable(share) { mutableStateOf(targets.firstOrNull()?.key) }
    var message by rememberSaveable(share) { mutableStateOf("") }
    val state = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val reachable = vm.feedUp
    ModalBottomSheet(onDismissRequest = onDone, sheetState = state, containerColor = Tm.Raised, contentColor = Tm.Text) {
        Column(Modifier.fillMaxWidth().imePadding().padding(start = 20.dp, end = 20.dp, bottom = 20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text("Share ${share.summary()} to a session", style = Tm.Body.copy(fontSize = 19.sp))
            share.text?.trim()?.takeIf { it.isNotEmpty() }?.let {
                Text(it, style = Tm.Small, maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
            when {
                !reachable -> Text("The tmls server can't be reached right now, so nothing can be shared. It is tried again by itself.",
                    style = Tm.Small.copy(color = Tm.Fail))
                targets.isEmpty() -> Text("No Claude session is running on a host that answers. Start one first.",
                    style = Tm.Small.copy(color = Tm.Fail))
                else -> LazyColumn(Modifier.heightIn(max = 300.dp)) {
                    items(targets, key = { it.key }) { row ->
                        val on = row.key == picked
                        Row(
                            Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(if (on) Tm.Bg else Tm.Raised)
                                .clickable(role = Role.RadioButton) { picked = row.key }.padding(horizontal = 12.dp, vertical = 10.dp),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(12.dp),
                        ) {
                            Box(Modifier.size(10.dp).clip(CircleShape).background(if (on) Tm.Accent else Tm.Line))
                            Column(Modifier.weight(1f)) {
                                Text(row.name, style = Tm.Body, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                Text(listOf(row.label, Sessions.state(row.mark)).filter { it.isNotEmpty() }
                                    .joinToString(" · "), style = Tm.Small, maxLines = 1)
                            }
                            Text(Sessions.glyph(row.mark), color = Tm.mark(row.mark))
                        }
                    }
                }
            }
            Surface(color = Tm.Bg, shape = RoundedCornerShape(16.dp), border = BorderStroke(1.dp, Tm.Line)) {
                BasicTextField(
                    value = message,
                    onValueChange = { message = it },
                    textStyle = Tm.Body,
                    cursorBrush = SolidColor(Tm.Accent),
                    maxLines = 4,
                    keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 14.dp, vertical = 10.dp),
                    decorationBox = { field ->
                        Box {
                            if (message.isEmpty()) Text("Message (optional)", style = Tm.Body.copy(color = Tm.Faint))
                            field()
                        }
                    },
                )
            }
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(10.dp, Alignment.End)) {
                OutlinedButton(onClick = onDone, border = BorderStroke(1.dp, Tm.Line)) { Text("Cancel", color = Tm.Text) }
                Spacer(Modifier.size(0.dp))
                val target = picked?.takeIf { key -> targets.any { it.key == key } }
                Button(enabled = reachable && target != null, onClick = {
                    if (target != null) vm.receiveShare(target, share, message)
                    onDone()
                }) { Text("Attach") }
            }
            Text("It goes into the session's message box; you send it from there.", style = Tm.Small.copy(fontSize = 13.sp))
        }
    }
}
