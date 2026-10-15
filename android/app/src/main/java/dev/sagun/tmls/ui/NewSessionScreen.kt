package dev.sagun.tmls.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.RadioButtonDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.sagun.tmls.chat.ChatViewModel
import dev.sagun.tmls.chat.HostInfo
import dev.sagun.tmls.chat.NewSession
import dev.sagun.tmls.chat.Sessions
import dev.sagun.tmls.chat.Start
import kotlinx.coroutines.delay

/**
 * New session, native: host, folder (the host's known folders offered), what to start, and the
 * name, which follows the folder (its git repo's name, from the server) until the user edits it.
 * Made sessions open as soon as the list has them; see ChatViewModel.submitCreate.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun NewSessionScreen(vm: ChatViewModel, host: HostInfo, modifier: Modifier = Modifier) {
    var folder by remember { mutableStateOf("~") }
    var name by remember { mutableStateOf(NewSession.defaultName("~")) }
    // the last name filled in for the user: while the field still holds it, it follows the folder
    var autoName by remember { mutableStateOf(name) }
    var picked by remember { mutableStateOf<String?>(null) }
    val starts = vm.starts
    val start: Start? = starts.firstOrNull { it.id == picked } ?: NewSession.defaultStart(starts)
    val known = vm.folders[host.host]

    BackHandler(onBack = vm::cancelCreate)

    LaunchedEffect(host.host, folder) {
        val auto = NewSession.defaultName(folder)
        if (name == autoName) name = auto
        autoName = auto
        delay(300)
        val suggested = vm.suggestName(host.host, folder.trim().ifEmpty { "~" }) ?: return@LaunchedEffect
        if (name == autoName) name = suggested
        autoName = suggested
    }

    Column(
        modifier.fillMaxSize().navigationBarsPadding().imePadding().verticalScroll(rememberScrollState())
            .padding(horizontal = 20.dp, vertical = 12.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        val hosts = Sessions.creatable(vm.hosts)
        if (hosts.size > 1) {
            Heading("Host")
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                hosts.forEach { h ->
                    FilterChip(
                        selected = h.host == host.host, onClick = { if (h.host != host.host) vm.create(h) },
                        label = { Text(h.label, fontSize = 15.sp) },
                        colors = FilterChipDefaults.filterChipColors(selectedContainerColor = Tm.Raised, selectedLabelColor = Tm.Text, labelColor = Tm.Dim),
                    )
                }
            }
        }

        Heading("Folder")
        OutlinedTextField(
            folder, { folder = it }, Modifier.fillMaxWidth(), singleLine = true, textStyle = Tm.Mono.copy(fontSize = 16.sp),
            keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.None, autoCorrectEnabled = false, keyboardType = KeyboardType.Uri, imeAction = ImeAction.Next),
            supportingText = known?.home?.takeIf { it.isNotEmpty() }?.let { { Text("~ is $it", style = Tm.Small.copy(fontSize = 12.sp, color = Tm.Faint)) } },
        )
        NewSession.choices(known).filter { it != folder.trim() }.take(MAX_FOLDERS).forEach { f ->
            Text(
                f, style = Tm.Mono.copy(color = Tm.Dim, fontSize = 14.sp), maxLines = 1, overflow = TextOverflow.Ellipsis,
                modifier = Modifier.fillMaxWidth().clickable(role = Role.Button) { folder = f }.padding(vertical = 7.dp, horizontal = 4.dp),
            )
        }

        Heading("Start")
        if (starts.isEmpty()) Text("Loading…", style = Tm.Small)
        starts.forEach { s ->
            Row(
                Modifier.fillMaxWidth().selectable(selected = s == start, role = Role.RadioButton) { picked = s.id }.padding(vertical = 2.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                RadioButton(selected = s == start, onClick = null, colors = RadioButtonDefaults.colors(selectedColor = Tm.Accent, unselectedColor = Tm.Faint))
                Column(Modifier.padding(start = 12.dp, top = 6.dp, bottom = 6.dp)) {
                    Text(NewSession.label(s), style = Tm.Body.copy(fontSize = 16.sp, lineHeight = 20.sp))
                    val command = s.command
                    if (command != null && command != s.id) {
                        Text(command, style = Tm.Mono.copy(fontSize = 12.sp, color = Tm.Faint), maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                }
            }
        }

        Heading("Name")
        OutlinedTextField(
            name, { name = it }, Modifier.fillMaxWidth(), singleLine = true, textStyle = Tm.Body.copy(fontSize = 16.sp),
            keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.None, autoCorrectEnabled = false, imeAction = ImeAction.Done),
        )

        vm.createError?.let { Text(it, style = Tm.Small.copy(color = Tm.Fail, fontSize = 15.sp), modifier = Modifier.padding(top = 4.dp)) }

        Row(Modifier.fillMaxWidth().padding(top = 8.dp), horizontalArrangement = Arrangement.End, verticalAlignment = Alignment.CenterVertically) {
            TextButton(onClick = vm::cancelCreate, enabled = !vm.createBusy) { Text("Cancel", color = Tm.Dim) }
            Button(
                onClick = { start?.let { vm.submitCreate(host, folder, name, it) } },
                enabled = start != null && !vm.createBusy,
                colors = ButtonDefaults.buttonColors(containerColor = Tm.Accent, contentColor = Tm.Text),
                modifier = Modifier.padding(start = 8.dp),
            ) {
                if (vm.createBusy) {
                    CircularProgressIndicator(Modifier.size(16.dp), color = Tm.Text, strokeWidth = 2.dp)
                    Text("Starting…", modifier = Modifier.padding(start = 8.dp))
                } else {
                    Text("Create")
                }
            }
        }
    }
}

private const val MAX_FOLDERS = 10

@Composable
private fun Heading(text: String) {
    Text(text, style = Tm.Small.copy(color = Tm.Faint, fontSize = 13.sp), modifier = Modifier.padding(top = 10.dp))
}
