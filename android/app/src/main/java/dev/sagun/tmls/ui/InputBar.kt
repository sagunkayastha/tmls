package dev.sagun.tmls.ui

import android.content.ActivityNotFoundException
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
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
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.FileProvider
import dev.sagun.tmls.chat.ChatViewModel
import dev.sagun.tmls.chat.Choice
import dev.sagun.tmls.chat.Picked
import dev.sagun.tmls.chat.Row as SessionRow
import dev.sagun.tmls.chat.Upload
import dev.sagun.tmls.chat.denyAt
import java.io.File

/** The most photos or files taken from one pick. */
private const val MAX_PICKED = 10

/**
 * Under the chat: what went wrong, an upload on its way, the permission prompt Claude waits on,
 * and the message box with + (camera, photo, file), Stop while Claude works, and send.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun Composer(vm: ChatViewModel, row: SessionRow) {
    val context = LocalContext.current
    var sheet by remember { mutableStateOf(false) }
    // The camera app writes into the app's cache (file_paths.xml); saved, so a photo taken while
    // Android had put the app away still arrives.
    var photo by rememberSaveable { mutableStateOf<String?>(null) }
    val takePicture = rememberLauncherForActivityResult(ActivityResultContracts.TakePicture()) { saved ->
        val file = photo?.let(::File)
        photo = null
        if (file != null) {
            if (saved) vm.attach(FileProvider.getUriForFile(context, "${context.packageName}.files", file), file.name, temp = file)
            else file.delete()
        }
    }
    val pickPhoto = rememberLauncherForActivityResult(ActivityResultContracts.PickMultipleVisualMedia(MAX_PICKED)) { uris ->
        val now = System.currentTimeMillis()
        vm.attach(uris.mapIndexed { i, uri -> Picked(uri, "photo-$now-${i + 1}.jpg") })
    }
    val pickFile = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris ->
        val now = System.currentTimeMillis()
        vm.attach(uris.take(MAX_PICKED).mapIndexed { i, uri -> Picked(uri, "file-$now-${i + 1}") })
    }

    Column(
        Modifier.fillMaxWidth().padding(start = 12.dp, end = 12.dp, bottom = 8.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        vm.error?.let { ErrorLine(it) { vm.error = null } }
        vm.upload?.let { UploadLine(it) }
        val prompt = row.prompt?.takeIf { row.key !in vm.answered }
        prompt?.let { PromptCard(it, row.choices, vm.answering, vm::answer) }
        if (row.waiting && row.prompt == null) {
            WaitingCard(row.waitingFor ?: "input needed", onShowTerminal = { vm.showTerminal(true) })
        }
        Surface(color = Tm.Raised, shape = RoundedCornerShape(26.dp), border = BorderStroke(1.dp, Tm.Line)) {
            Column(Modifier.padding(8.dp)) {
                BasicTextField(
                    value = vm.draft,
                    onValueChange = { vm.draft = it },
                    textStyle = Tm.Body,
                    cursorBrush = SolidColor(Tm.Accent),
                    maxLines = 7,
                    keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 10.dp),
                    decorationBox = { field ->
                        Box {
                            if (vm.draft.text.isEmpty()) Text(if (row.asking) "Answer the prompt above first" else "Message", style = Tm.Body.copy(color = Tm.Faint))
                            field()
                        }
                    },
                )
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    RoundButton(Glyphs.Plus, "Attach: camera, photo or file", Tm.Bg, Tm.Text, enabled = vm.upload == null) { sheet = true }
                    if (row.working) {
                        Row(
                            Modifier.clip(CircleShape).background(Tm.Bg)
                                .clickable(enabled = !vm.stopping, role = Role.Button, onClickLabel = "Stop Claude", onClick = vm::stop)
                                .padding(start = 12.dp, end = 16.dp).heightIn(min = 44.dp),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(6.dp),
                        ) {
                            Icon(Glyphs.Stop, null, Modifier.size(18.dp), tint = Tm.Accent)
                            Text("Stop", style = Tm.Small.copy(color = Tm.Text, fontSize = 15.sp))
                        }
                    }
                    Spacer(Modifier.weight(1f))
                    val ready = vm.draft.text.isNotBlank() && !vm.sending && !row.asking
                    Box(
                        Modifier.size(44.dp).clip(CircleShape).background(if (ready || vm.sending) Tm.Accent else Tm.Accent.copy(alpha = 0.35f))
                            .clickable(enabled = ready, role = Role.Button, onClickLabel = "Send", onClick = vm::send),
                        contentAlignment = Alignment.Center,
                    ) {
                        if (vm.sending) CircularProgressIndicator(Modifier.size(20.dp), color = Color.White, strokeWidth = 2.dp)
                        else Icon(Glyphs.Up, "Send", Modifier.size(22.dp), tint = Color.White)
                    }
                }
            }
        }
    }

    if (sheet) {
        ModalBottomSheet(onDismissRequest = { sheet = false }, containerColor = Tm.Raised, contentColor = Tm.Text) {
            Column(Modifier.padding(bottom = 16.dp)) {
                SheetRow(Glyphs.Camera, "Camera") {
                    sheet = false
                    val file = File(File(context.cacheDir, "camera").apply { mkdirs() }, "photo-${System.currentTimeMillis()}.jpg")
                    photo = file.path
                    try {
                        takePicture.launch(FileProvider.getUriForFile(context, "${context.packageName}.files", file))
                    } catch (e: ActivityNotFoundException) {
                        photo = null
                        vm.error = "No camera app on this phone."
                    }
                }
                SheetRow(Glyphs.Photo, "Photo") {
                    sheet = false
                    try {
                        pickPhoto.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly))
                    } catch (e: ActivityNotFoundException) {
                        vm.error = "No photo picker on this phone."
                    }
                }
                SheetRow(Glyphs.File, "File") {
                    sheet = false
                    try {
                        pickFile.launch(arrayOf("*/*"))
                    } catch (e: ActivityNotFoundException) {
                        vm.error = "No file picker on this phone."
                    }
                }
            }
        }
    }
}

@Composable
private fun RoundButton(icon: ImageVector, label: String, color: Color, tint: Color, enabled: Boolean, onClick: () -> Unit) {
    Box(
        Modifier.size(44.dp).clip(CircleShape).background(color).clickable(enabled = enabled, role = Role.Button, onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        Icon(icon, label, Modifier.size(22.dp), tint = if (enabled) tint else Tm.Faint)
    }
}

@Composable
private fun SheetRow(icon: ImageVector, label: String, onClick: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().clickable(role = Role.Button, onClick = onClick).padding(horizontal = 28.dp, vertical = 16.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(20.dp),
    ) {
        Icon(icon, null, Modifier.size(24.dp), tint = Tm.Dim)
        Text(label, style = Tm.Body)
    }
}

/** What went wrong, until tapped away. */
@Composable
private fun ErrorLine(text: String, onDismiss: () -> Unit) {
    Surface(color = Tm.Raised, shape = RoundedCornerShape(14.dp), border = BorderStroke(1.dp, Tm.Fail.copy(alpha = 0.5f))) {
        Row(Modifier.fillMaxWidth().clickable(onClickLabel = "Dismiss", onClick = onDismiss).padding(horizontal = 16.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically) {
            Text(text, style = Tm.Small.copy(color = Tm.Fail), maxLines = 4, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
            Icon(Glyphs.Close, null, Modifier.padding(start = 10.dp).size(16.dp), tint = Tm.Dim)
        }
    }
}

@Composable
private fun UploadLine(upload: Upload) {
    Column(Modifier.fillMaxWidth().padding(horizontal = 8.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        val percent = upload.fraction?.let { " ${(it * 100).toInt()}%" } ?: ""
        Text("Uploading ${upload.name}$percent", style = Tm.Small, maxLines = 1, overflow = TextOverflow.Ellipsis)
        val fraction = upload.fraction
        if (fraction == null) LinearProgressIndicator(Modifier.fillMaxWidth(), color = Tm.Accent, trackColor = Tm.Line)
        else LinearProgressIndicator(progress = { fraction }, modifier = Modifier.fillMaxWidth(), color = Tm.Accent, trackColor = Tm.Line)
    }
}

/**
 * The permission prompt Claude waits on, as the terminal shows it, with its answers: one button per
 * numbered choice when the server lists them, else Allow and Deny.
 */
@Composable
private fun PromptCard(lines: List<String>, choices: List<Choice>?, busy: Boolean, onAnswer: (Boolean, Int?) -> Unit) {
    Surface(color = Tm.Raised, shape = RoundedCornerShape(18.dp), border = BorderStroke(1.dp, Tm.Line)) {
        Column(Modifier.fillMaxWidth().padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text("Claude asks for permission", style = Tm.Small)
            if (lines.isNotEmpty()) {
                Column(Modifier.heightIn(max = 180.dp).verticalScroll(rememberScrollState())) {
                    lines.forEach { Text(it, style = Tm.Mono.copy(fontSize = 13.sp, lineHeight = 18.sp)) }
                }
            }
            if (busy) CircularProgressIndicator(Modifier.size(18.dp), color = Tm.Accent, strokeWidth = 2.dp)
            if (choices.isNullOrEmpty()) {
                Row(horizontalArrangement = Arrangement.spacedBy(10.dp, Alignment.End), modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically) {
                    OutlinedButton(onClick = { onAnswer(false, null) }, enabled = !busy, border = BorderStroke(1.dp, Tm.Line)) { Text("Deny", color = Tm.Text) }
                    Button(onClick = { onAnswer(true, null) }, enabled = !busy) { Text("Allow") }
                }
            } else {
                val deny = denyAt(choices)
                choices.forEachIndexed { i, choice ->
                    when {
                        i == 0 -> Button(onClick = { onAnswer(true, choice.n) }, enabled = !busy, modifier = Modifier.fillMaxWidth()) { Text("Allow") }
                        i == deny ->
                            OutlinedButton(onClick = { onAnswer(false, null) }, enabled = !busy, border = BorderStroke(1.dp, Tm.Line),
                                modifier = Modifier.fillMaxWidth()) { Text("Deny", color = Tm.Text) }
                        else -> OutlinedButton(onClick = { onAnswer(true, choice.n) }, enabled = !busy, border = BorderStroke(1.dp, Tm.Line),
                            modifier = Modifier.fillMaxWidth()) { Text(choice.label, color = Tm.Text, maxLines = 3, overflow = TextOverflow.Ellipsis) }
                    }
                }
                if (deny == null) {  // the prompt lists no "No": Escape still denies
                    OutlinedButton(onClick = { onAnswer(false, null) }, enabled = !busy, border = BorderStroke(1.dp, Tm.Line),
                        modifier = Modifier.fillMaxWidth()) { Text("Deny", color = Tm.Text) }
                }
            }
        }
    }
}

/** Claude waits on a menu or a question the app cannot draw: say so, and offer the terminal. */
@Composable
private fun WaitingCard(reason: String, onShowTerminal: () -> Unit) {
    Surface(color = Tm.Raised, shape = RoundedCornerShape(18.dp), border = BorderStroke(1.dp, Tm.Line)) {
        Column(Modifier.fillMaxWidth().padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text("Claude is waiting: $reason", style = Tm.Small)
            OutlinedButton(onClick = onShowTerminal, border = BorderStroke(1.dp, Tm.Line)) { Text("Show terminal", color = Tm.Text) }
        }
    }
}
